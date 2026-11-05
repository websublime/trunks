//! Access model (D14): single-tenant teams, resource-side grants, owned MCP clients.
//!
//! Everything here is pure: the stores hand in rows, this module resolves what a principal
//! may use. Fail closed — an empty set is the default for anything unknown.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use trunks_types::{CanonicalName, GroupName, ServerName};

use crate::catalog::Snapshot;

/// Who a grant or an ownership points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectKind {
    User,
    Team,
}

impl SubjectKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Team => "team",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "user" => Some(Self::User),
            "team" => Some(Self::Team),
            _ => None,
        }
    }
}

/// What a grant targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Server,
    ToolGroup,
}

impl TargetKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::ToolGroup => "tool_group",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "server" => Some(Self::Server),
            "tool_group" => Some(Self::ToolGroup),
            _ => None,
        }
    }
}

/// Role inside a team.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TeamRole {
    Maintainer,
    #[default]
    Member,
}

impl TeamRole {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Maintainer => "maintainer",
            Self::Member => "member",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "maintainer" => Some(Self::Maintainer),
            "member" => Some(Self::Member),
            _ => None,
        }
    }
}

/// One stored grant, already joined to names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub target_kind: TargetKind,
    /// A server name, a tool group name, or `*` (every server).
    pub target_name: String,
}

/// What a principal may use. `all` short-circuits everything (admin, dev mode, `*` grant).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessSet {
    pub all: bool,
    pub servers: BTreeSet<ServerName>,
    pub tool_groups: BTreeSet<GroupName>,
}

impl AccessSet {
    #[must_use]
    pub fn everything() -> Self {
        Self { all: true, ..Default::default() }
    }

    #[must_use]
    pub fn nothing() -> Self {
        Self::default()
    }

    /// Builds the set from a subject's grants (its own plus its teams'). Unknown names are
    /// kept — they may refer to resources registered later.
    #[must_use]
    pub fn from_grants<'a>(grants: impl IntoIterator<Item = &'a Grant>) -> Self {
        let mut set = Self::default();
        for g in grants {
            match g.target_kind {
                TargetKind::Server if g.target_name == "*" => set.all = true,
                TargetKind::Server => {
                    if let Ok(n) = ServerName::parse(g.target_name.clone()) {
                        set.servers.insert(n);
                    }
                }
                TargetKind::ToolGroup => {
                    if let Ok(n) = GroupName::parse(&g.target_name) {
                        set.tool_groups.insert(n);
                    }
                }
            }
        }
        set
    }

    /// The legacy `allow_list` of an MCP client as a set (`*` = everything, empty = nothing).
    #[must_use]
    pub fn from_allow_list(allow_list: &[String]) -> Self {
        let mut set = Self::default();
        for a in allow_list {
            if a == "*" {
                set.all = true;
            } else if let Ok(n) = ServerName::parse(a.clone()) {
                set.servers.insert(n);
            }
        }
        set
    }

    /// `owner ∩ allow_list` for an owned MCP client: an empty allow list means "everything the
    /// owner can"; otherwise the client is narrowed to the listed servers.
    #[must_use]
    pub fn narrowed_by(&self, allow_list: &[String]) -> Self {
        if allow_list.is_empty() {
            return self.clone();
        }
        let narrow = Self::from_allow_list(allow_list);
        if narrow.all {
            return self.clone();
        }
        if self.all {
            return Self { all: false, servers: narrow.servers, tool_groups: BTreeSet::new() };
        }
        Self {
            all: false,
            servers: self.servers.intersection(&narrow.servers).cloned().collect(),
            // a narrowed client keeps only direct server access; groups would re-widen it
            tool_groups: BTreeSet::new(),
        }
    }

    #[must_use]
    pub fn may_use_server(&self, server: &ServerName) -> bool {
        self.all || self.servers.contains(server)
    }

    #[must_use]
    pub fn may_use_group(&self, group: &GroupName) -> bool {
        self.all || self.tool_groups.contains(group)
    }

    /// A tool is usable through a granted server or through a granted group that resolves to it.
    #[must_use]
    pub fn may_use_tool(&self, snap: &Snapshot, canonical: &CanonicalName) -> bool {
        if self.may_use_server(canonical.server()) {
            return true;
        }
        self.tool_groups
            .iter()
            .filter_map(|g| snap.groups.get(g))
            .any(|g| g.effective_tools.contains(canonical))
    }

    /// Empty for a subject with no usable grants (used to short-circuit listings).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.all && self.servers.is_empty() && self.tool_groups.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(kind: TargetKind, name: &str) -> Grant {
        Grant { target_kind: kind, target_name: name.into() }
    }

    fn s(n: &str) -> ServerName {
        ServerName::parse(n).unwrap()
    }

    #[test]
    fn grants_resolve_to_sets() {
        type Case = (Vec<Grant>, bool, Vec<&'static str>, Vec<&'static str>);
        let cases: Vec<Case> = vec![
            (vec![], false, vec![], vec![]),
            (vec![g(TargetKind::Server, "a")], false, vec!["a"], vec![]),
            (vec![g(TargetKind::Server, "*")], true, vec![], vec![]),
            (
                vec![g(TargetKind::ToolGroup, "dev"), g(TargetKind::Server, "b")],
                false,
                vec!["b"],
                vec!["dev"],
            ),
            (vec![g(TargetKind::Server, "bad__name")], false, vec![], vec![]),
        ];
        for (grants, all, servers, groups) in cases {
            let set = AccessSet::from_grants(&grants);
            assert_eq!(set.all, all, "{grants:?}");
            assert_eq!(set.servers.iter().map(ToString::to_string).collect::<Vec<_>>(), servers);
            assert_eq!(set.tool_groups.iter().map(ToString::to_string).collect::<Vec<_>>(), groups);
        }
    }

    #[test]
    fn owned_client_is_narrowed_never_widened() {
        let owner = AccessSet::from_grants(&[
            g(TargetKind::Server, "a"),
            g(TargetKind::Server, "b"),
            g(TargetKind::ToolGroup, "dev"),
        ]);
        // empty allow list = owner's full set
        assert_eq!(owner.narrowed_by(&[]), owner);
        // wildcard = owner's full set
        assert_eq!(owner.narrowed_by(&["*".into()]), owner);
        // narrowing to b (and c the owner lacks): only b, groups dropped
        let n = owner.narrowed_by(&["b".into(), "c".into()]);
        assert!(
            !n.all
                && n.may_use_server(&s("b"))
                && !n.may_use_server(&s("a"))
                && !n.may_use_server(&s("c"))
        );
        assert!(n.tool_groups.is_empty());
        // an admin-like owner narrowed to one server
        let n = AccessSet::everything().narrowed_by(&["x".into()]);
        assert!(!n.all && n.may_use_server(&s("x")) && !n.may_use_server(&s("y")));
        // nothing stays nothing
        assert!(AccessSet::nothing().narrowed_by(&["x".into()]).is_empty());
    }
}
