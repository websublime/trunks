//! Tool groups: validation against the catalog, diff, CRUD (`docs/02-especificacao.md` §4.10).
//!
//! A group is stored as written (names, not ids) and resolved against the current snapshot on
//! every rebuild, so a group whose inputs disappear degrades to empty instead of failing.

use std::collections::BTreeSet;
use std::sync::Arc;

use trunks_types::api::ToolGroup;
use trunks_types::error::messages;
use trunks_types::{CanonicalName, GroupName, ServerName};

use crate::catalog::{Catalog, GroupView, Snapshot};
use crate::ports::{RegistryStore, ToolGroupSpec};
use crate::{Error, Result};

#[derive(Clone)]
pub struct GroupService {
    store: Arc<dyn RegistryStore>,
    catalog: Catalog,
}

impl std::fmt::Debug for GroupService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GroupService").finish_non_exhaustive()
    }
}

/// What changed between two versions of a group, per list field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupDiff {
    pub added_tools: Vec<String>,
    pub removed_tools: Vec<String>,
    pub added_servers: Vec<String>,
    pub removed_servers: Vec<String>,
    pub added_exclusions: Vec<String>,
    pub removed_exclusions: Vec<String>,
}

impl GroupDiff {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added_tools.is_empty()
            && self.removed_tools.is_empty()
            && self.added_servers.is_empty()
            && self.removed_servers.is_empty()
            && self.added_exclusions.is_empty()
            && self.removed_exclusions.is_empty()
    }
}

/// Set difference per field, in lexicographic order.
#[must_use]
pub fn diff(old: &ToolGroup, new: &ToolGroup) -> GroupDiff {
    fn delta(old: &[String], new: &[String]) -> (Vec<String>, Vec<String>) {
        let o: BTreeSet<&String> = old.iter().collect();
        let n: BTreeSet<&String> = new.iter().collect();
        (
            n.difference(&o).map(|s| (*s).clone()).collect(),
            o.difference(&n).map(|s| (*s).clone()).collect(),
        )
    }
    let (added_tools, removed_tools) = delta(&old.included_tools, &new.included_tools);
    let (added_servers, removed_servers) = delta(&old.included_servers, &new.included_servers);
    let (added_exclusions, removed_exclusions) = delta(&old.excluded_tools, &new.excluded_tools);
    GroupDiff {
        added_tools,
        removed_tools,
        added_servers,
        removed_servers,
        added_exclusions,
        removed_exclusions,
    }
}

/// The public view of a stored group.
#[must_use]
pub fn to_api(view: &GroupView) -> ToolGroup {
    ToolGroup {
        name: view.name.to_string(),
        description: view.description.clone(),
        included_tools: view.included_tools.clone(),
        included_servers: view.included_servers.clone(),
        excluded_tools: view.excluded_tools.clone(),
    }
}

/// Path of a group's MCP endpoint, relative to the server.
#[must_use]
pub fn endpoint(name: &GroupName) -> String {
    format!("/v0/groups/{name}/mcp")
}

/// Validates a group against the snapshot: the name, every included tool (must exist and be
/// enabled), every included server (must exist), every exclusion (must parse), and a
/// non-empty effective set. Lists are de-duplicated and sorted.
pub fn validate(snap: &Snapshot, input: &ToolGroup) -> Result<ToolGroupSpec> {
    let name = GroupName::parse(input.name.trim())?;
    let mut included_tools = BTreeSet::new();
    for t in &input.included_tools {
        let canonical = CanonicalName::parse(t)?;
        if !snap.tools.get(&canonical).is_some_and(|e| e.enabled) {
            return Err(Error::invalid_input(format!("tool '{t}' does not exist or is disabled")));
        }
        included_tools.insert(canonical.to_string());
    }
    let mut included_servers = BTreeSet::new();
    for s in &input.included_servers {
        let server = ServerName::parse(s.clone())?;
        if !snap.servers.contains_key(&server) {
            return Err(Error::invalid_input(format!("server '{s}' does not exist")));
        }
        included_servers.insert(server.to_string());
    }
    let mut excluded_tools = BTreeSet::new();
    for t in &input.excluded_tools {
        excluded_tools.insert(CanonicalName::parse(t)?.to_string());
    }
    let spec = ToolGroupSpec {
        name,
        description: input.description.trim().to_owned(),
        included_tools: included_tools.into_iter().collect(),
        included_servers: included_servers.into_iter().collect(),
        excluded_tools: excluded_tools.into_iter().collect(),
    };
    let effective = crate::catalog::resolve_effective(
        &snap.tools,
        &spec.included_tools,
        &spec.included_servers,
        &spec.excluded_tools,
    );
    if effective.is_empty() {
        return Err(Error::invalid_input(messages::GROUP_NO_TOOLS));
    }
    Ok(spec)
}

impl GroupService {
    #[must_use]
    pub fn new(store: Arc<dyn RegistryStore>, catalog: Catalog) -> Self {
        Self { store, catalog }
    }

    pub async fn create(&self, input: &ToolGroup) -> Result<Arc<GroupView>> {
        let snap = self.catalog.load();
        let spec = validate(&snap, input)?;
        if snap.groups.contains_key(&spec.name) {
            return Err(Error::invalid_input(format!("tool group '{}' already exists", spec.name)));
        }
        self.store.create_group(&spec).await?;
        let snap = self.catalog.reload(self.store.as_ref()).await?;
        Self::view(&snap, &spec.name)
    }

    /// Replaces the whole group (empty lists included). Returns `(old, new)` views.
    pub async fn update(
        &self,
        name: &GroupName,
        input: &ToolGroup,
    ) -> Result<(ToolGroup, Arc<GroupView>)> {
        let snap = self.catalog.load();
        let old = Self::view(&snap, name)?;
        let mut input = input.clone();
        input.name = name.to_string();
        let spec = validate(&snap, &input)?;
        if !self.store.update_group(&spec).await? {
            return Err(Error::not_found(format!("tool group '{name}'")));
        }
        let snap = self.catalog.reload(self.store.as_ref()).await?;
        Ok((to_api(&old), Self::view(&snap, name)?))
    }

    /// Idempotent: deleting an unknown group is not an error.
    pub async fn delete(&self, name: &GroupName) -> Result<()> {
        if self.store.delete_group(name).await? {
            self.catalog.reload(self.store.as_ref()).await?;
        }
        Ok(())
    }

    fn view(snap: &Snapshot, name: &GroupName) -> Result<Arc<GroupView>> {
        snap.groups
            .get(name)
            .cloned()
            .ok_or_else(|| Error::not_found(format!("tool group '{name}'")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(tools: &[&str], servers: &[&str], excluded: &[&str]) -> ToolGroup {
        ToolGroup {
            name: "g".into(),
            description: String::new(),
            included_tools: tools.iter().map(|s| (*s).to_owned()).collect(),
            included_servers: servers.iter().map(|s| (*s).to_owned()).collect(),
            excluded_tools: excluded.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn diff_is_per_field_and_sorted() {
        let old = g(&["a__x", "a__y"], &["b"], &[]);
        let new = g(&["a__y", "a__z"], &[], &["b__q"]);
        let d = diff(&old, &new);
        assert_eq!(d.added_tools, vec!["a__z"]);
        assert_eq!(d.removed_tools, vec!["a__x"]);
        assert_eq!(d.removed_servers, vec!["b"]);
        assert!(d.added_servers.is_empty());
        assert_eq!(d.added_exclusions, vec!["b__q"]);
        assert!(diff(&old, &old).is_empty());
    }

    #[test]
    fn endpoint_path() {
        assert_eq!(endpoint(&GroupName::parse("dev").unwrap()), "/v0/groups/dev/mcp");
    }
}
