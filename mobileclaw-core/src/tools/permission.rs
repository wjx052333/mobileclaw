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

    pub fn check(&self, perm: &Permission) -> bool {
        self.allowed.contains(perm)
    }
}
