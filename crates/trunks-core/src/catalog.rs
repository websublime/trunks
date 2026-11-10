//! In-memory catalog: an immutable snapshot of the registry, rebuilt after each mutation,
//! read lock-free by the gateway and the REST handlers.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use arc_swap::ArcSwap;
use trunks_types::{CanonicalName, GroupName, ResourceUri, ServerName};

use crate::domain::{PromptEntry, ResourceEntry, ServerRecord, ToolEntry};
use crate::ports::{RegistrySnapshot, RegistryStore};
use crate::{Error, Result};

/// A tool group resolved against the snapshot.
#[derive(Debug, Clone)]
pub struct GroupView {
    pub name: GroupName,
    pub description: String,
    pub included_tools: Vec<String>,
    pub included_servers: Vec<String>,
    pub excluded_tools: Vec<String>,
    /// Canonical names present **and enabled** in the snapshot, sorted. Empty for a degraded group.
    pub effective_tools: BTreeSet<CanonicalName>,
}

/// Immutable view of everything registered. Includes disabled entities (flagged) so REST
/// listings can show them; the gateway filters with `enabled`.
#[derive(Debug, Default)]
pub struct Snapshot {
    pub servers: BTreeMap<ServerName, Arc<ServerRecord>>,
    pub tools: BTreeMap<CanonicalName, Arc<ToolEntry>>,
    pub prompts: BTreeMap<CanonicalName, Arc<PromptEntry>>,
    pub resources: BTreeMap<ResourceUri, Arc<ResourceEntry>>,
    pub groups: BTreeMap<GroupName, Arc<GroupView>>,
}

impl Snapshot {
    /// Builds the snapshot. Entities whose server is missing are dropped with a warning; a group
    /// whose inputs cannot be resolved is kept **empty** (degraded) rather than failing startup.
    pub fn build(src: RegistrySnapshot) -> Result<Self> {
        let mut by_id: HashMap<i64, Arc<ServerRecord>> = HashMap::new();
        let mut servers = BTreeMap::new();
        for s in src.servers {
            let s = Arc::new(s);
            by_id.insert(s.id, s.clone());
            servers.insert(s.name.clone(), s);
        }

        let mut tools = BTreeMap::new();
        for t in src.tools {
            let Some(server) = by_id.get(&t.server_id) else {
                tracing::warn!(tool = %t.name, server_id = t.server_id, "tool references a missing server; skipped");
                continue;
            };
            let canonical = CanonicalName::new(server.name.clone(), t.name);
            tools.insert(
                canonical.clone(),
                Arc::new(ToolEntry {
                    canonical,
                    server: server.name.clone(),
                    enabled: t.enabled && server.enabled,
                    description: t.description,
                    input_schema: t.input_schema,
                    annotations: t.annotations,
                }),
            );
        }

        let mut prompts = BTreeMap::new();
        for p in src.prompts {
            let Some(server) = by_id.get(&p.server_id) else {
                tracing::warn!(prompt = %p.name, server_id = p.server_id, "prompt references a missing server; skipped");
                continue;
            };
            let canonical = CanonicalName::new(server.name.clone(), p.name);
            prompts.insert(
                canonical.clone(),
                Arc::new(PromptEntry {
                    canonical,
                    server: server.name.clone(),
                    enabled: p.enabled && server.enabled,
                    description: p.description,
                    arguments: p.arguments,
                }),
            );
        }

        let mut resources = BTreeMap::new();
        for r in src.resources {
            let Some(server) = by_id.get(&r.server_id) else {
                tracing::warn!(uri = %r.uri, server_id = r.server_id, "resource references a missing server; skipped");
                continue;
            };
            resources.insert(
                r.uri.clone(),
                Arc::new(ResourceEntry {
                    uri: r.uri,
                    server: server.name.clone(),
                    original_uri: r.original_uri,
                    name: r.name,
                    enabled: r.enabled && server.enabled,
                    description: r.description,
                    mime_type: r.mime_type,
                    annotations: r.annotations,
                    meta: r.meta,
                }),
            );
        }

        let mut groups = BTreeMap::new();
        for g in src.groups {
            let name = match GroupName::parse(&g.name) {
                Ok(n) => n,
                Err(e) => {
                    tracing::warn!(group = %g.name, error = %e, "invalid group name in store; skipped");
                    continue;
                }
            };
            let effective = resolve_effective(
                &tools,
                &g.included_tools,
                &g.included_servers,
                &g.excluded_tools,
            );
            if effective.is_empty() {
                tracing::warn!(group = %name, "tool group resolves to no enabled tools (degraded)");
            }
            groups.insert(
                name.clone(),
                Arc::new(GroupView {
                    name,
                    description: g.description,
                    included_tools: g.included_tools,
                    included_servers: g.included_servers,
                    excluded_tools: g.excluded_tools,
                    effective_tools: effective,
                }),
            );
        }

        Ok(Self { servers, tools, prompts, resources, groups })
    }

    pub fn server(&self, name: &ServerName) -> Result<&Arc<ServerRecord>> {
        self.servers.get(name).ok_or_else(|| Error::not_found(format!("mcp server '{name}'")))
    }

    pub fn enabled_tools(&self) -> impl Iterator<Item = &Arc<ToolEntry>> {
        self.tools.values().filter(|t| t.enabled)
    }

    pub fn enabled_prompts(&self) -> impl Iterator<Item = &Arc<PromptEntry>> {
        self.prompts.values().filter(|p| p.enabled)
    }

    pub fn enabled_resources(&self) -> impl Iterator<Item = &Arc<ResourceEntry>> {
        self.resources.values().filter(|r| r.enabled)
    }
}

/// `included_tools ∪ ⋃ tools(included_servers) − excluded_tools`, restricted to enabled tools.
/// Exclusion always wins.
pub(crate) fn resolve_effective(
    tools: &BTreeMap<CanonicalName, Arc<ToolEntry>>,
    included_tools: &[String],
    included_servers: &[String],
    excluded_tools: &[String],
) -> BTreeSet<CanonicalName> {
    let mut set: BTreeSet<CanonicalName> = BTreeSet::new();
    for name in included_tools {
        if let Ok(c) = CanonicalName::parse(name)
            && tools.get(&c).is_some_and(|t| t.enabled)
        {
            set.insert(c);
        }
    }
    for server in included_servers {
        for t in tools.values() {
            if t.enabled && t.server.as_str() == server {
                set.insert(t.canonical.clone());
            }
        }
    }
    for name in excluded_tools {
        if let Ok(c) = CanonicalName::parse(name) {
            set.remove(&c);
        }
    }
    set
}

/// Shared, lock-free handle to the current snapshot.
#[derive(Debug, Clone)]
pub struct Catalog(Arc<ArcSwap<Snapshot>>);

impl Catalog {
    #[must_use]
    pub fn new(snapshot: Snapshot) -> Self {
        Self(Arc::new(ArcSwap::from_pointee(snapshot)))
    }

    #[must_use]
    pub fn empty() -> Self {
        Self::new(Snapshot::default())
    }

    /// Loads a fresh snapshot from the store and publishes it.
    pub async fn reload(&self, store: &dyn RegistryStore) -> Result<Arc<Snapshot>> {
        let snap = Arc::new(Snapshot::build(store.load_snapshot().await?)?);
        self.0.store(snap.clone());
        Ok(snap)
    }

    /// Current snapshot (cheap, lock-free).
    #[must_use]
    pub fn load(&self) -> Arc<Snapshot> {
        self.0.load_full()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ToolGroupRecord, ToolRecord, UpstreamConfig};
    use trunks_types::SessionMode;

    fn server(id: i64, name: &str, enabled: bool) -> ServerRecord {
        ServerRecord {
            id,
            name: ServerName::parse(name).unwrap(),
            enabled,
            description: String::new(),
            config: UpstreamConfig::StreamableHttp {
                url: "http://x".into(),
                bearer_token: None,
                headers: BTreeMap::default(),
            },
            session_mode: SessionMode::Stateless,
            created_at: None,
            updated_at: None,
        }
    }
    fn tool(server_id: i64, name: &str, enabled: bool) -> ToolRecord {
        ToolRecord {
            id: 0,
            server_id,
            name: name.into(),
            enabled,
            description: String::new(),
            input_schema: None,
            annotations: None,
        }
    }

    #[test]
    fn builds_canonical_names_and_cascades_server_disable() {
        let snap = Snapshot::build(RegistrySnapshot {
            servers: vec![server(1, "fs", true), server(2, "off", false)],
            tools: vec![
                tool(1, "read", true),
                tool(1, "write", false),
                tool(2, "x", true),
                tool(9, "orphan", true),
            ],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(snap.tools.len(), 3, "orphan dropped");
        assert!(snap.tools[&CanonicalName::parse("fs__read").unwrap()].enabled);
        assert!(!snap.tools[&CanonicalName::parse("fs__write").unwrap()].enabled);
        assert!(
            !snap.tools[&CanonicalName::parse("off__x").unwrap()].enabled,
            "disabled server disables its tools"
        );
        assert_eq!(snap.enabled_tools().count(), 1);
    }

    #[test]
    fn groups_resolve_with_exclusion_winning_and_degrade_gracefully() {
        let group = |name: &str, it: &[&str], is: &[&str], ex: &[&str]| ToolGroupRecord {
            id: 0,
            name: name.into(),
            description: String::new(),
            included_tools: it.iter().map(|s| (*s).to_owned()).collect(),
            included_servers: is.iter().map(|s| (*s).to_owned()).collect(),
            excluded_tools: ex.iter().map(|s| (*s).to_owned()).collect(),
        };
        let snap = Snapshot::build(RegistrySnapshot {
            servers: vec![server(1, "fs", true), server(2, "gh", true)],
            tools: vec![
                tool(1, "read", true),
                tool(1, "write", false),
                tool(2, "search", true),
                tool(2, "issues", true),
            ],
            groups: vec![
                group("mix", &["fs__read", "fs__write"], &["gh"], &["gh__issues"]),
                group("both", &["gh__search"], &[], &["gh__search"]),
                group("gone", &["nope__tool"], &["missing"], &[]),
            ],
            ..Default::default()
        })
        .unwrap();
        let names = |g: &str| -> Vec<String> {
            snap.groups[&GroupName::parse(g).unwrap()]
                .effective_tools
                .iter()
                .map(ToString::to_string)
                .collect()
        };
        assert_eq!(
            names("mix"),
            ["fs__read", "gh__search"],
            "disabled tool omitted, server expanded, exclusion applied"
        );
        assert!(names("both").is_empty(), "exclusion beats explicit inclusion");
        assert!(names("gone").is_empty(), "degraded group kept, empty");
    }
}
