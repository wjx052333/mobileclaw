use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Permission {
    FileRead,
    FileWrite,
    HttpFetch,
    MemoryRead,
    MemoryWrite,
    SystemInfo,
    Notifications,
}

pub struct PermissionChecker {
    allowed: std::collections::HashSet<Permission>,
}

impl PermissionChecker {
    pub fn allow_all() -> Self {
        use Permission::*;
        Self {
            allowed: [
                FileRead,
                FileWrite,
                HttpFetch,
                MemoryRead,
                MemoryWrite,
                SystemInfo,
                Notifications,
            ]
            .into_iter()
            .collect(),
        }
    }

    pub fn new(allowed: impl IntoIterator<Item = Permission>) -> Self {
        Self { allowed: allowed.into_iter().collect() }
    }

    pub fn check(&self, perm: &Permission) -> bool {
        self.allowed.contains(perm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_all_grants_every_permission() {
        let checker = PermissionChecker::allow_all();
        assert!(checker.check(&Permission::FileRead));
        assert!(checker.check(&Permission::FileWrite));
        assert!(checker.check(&Permission::HttpFetch));
        assert!(checker.check(&Permission::MemoryRead));
        assert!(checker.check(&Permission::MemoryWrite));
        assert!(checker.check(&Permission::SystemInfo));
        assert!(checker.check(&Permission::Notifications));
    }

    #[test]
    fn selective_permissions_deny_unlisted() {
        let checker = PermissionChecker::new([Permission::FileRead]);
        assert!(checker.check(&Permission::FileRead));
        assert!(!checker.check(&Permission::FileWrite));
        assert!(!checker.check(&Permission::HttpFetch));
    }

    #[test]
    fn empty_checker_denies_all() {
        let checker = PermissionChecker::new([]);
        assert!(!checker.check(&Permission::FileRead));
        assert!(!checker.check(&Permission::MemoryWrite));
    }
}
