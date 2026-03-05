pub mod auth;
pub mod context;
pub mod fallback;
pub mod hash;
pub mod models;
pub mod providers;

/// The fingerprint of the user's machine + intent, used as the Bloom filter key.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SystemState {
    /// Normalised intent, e.g. "install_neofetch"
    pub intent: String,
    /// Operating system, e.g. "arch_linux", "macos"
    pub os: String,
    /// CPU architecture, e.g. "x86_64", "aarch64"
    pub arch: String,
    /// Detected package managers, e.g. ["pacman", "yay"]
    pub package_managers: Vec<String>,
    /// GPU driver if relevant (gaming, CUDA, etc.)
    pub gpu_driver: Option<String>,
    /// User's shell, e.g. "zsh", "bash"
    pub shell: String,
    /// Error logs accumulated across Fixer retries (starts empty)
    pub errors_encountered: Vec<String>,
}

impl SystemState {
    /// Create a new SystemState with empty errors.
    pub fn new(
        intent: String,
        os: String,
        arch: String,
        package_managers: Vec<String>,
        gpu_driver: Option<String>,
        shell: String,
    ) -> Self {
        Self {
            intent,
            os,
            arch,
            package_managers,
            gpu_driver,
            shell,
            errors_encountered: Vec::new(),
        }
    }

    /// Produce the composite string used for hashing.
    /// Errors are excluded — they're runtime context, not cache keys.
    pub fn fingerprint(&self) -> String {
        let mut parts = vec![
            self.intent.clone(),
            self.os.clone(),
            self.arch.clone(),
            self.shell.clone(),
        ];
        parts.extend(self.package_managers.iter().cloned());
        if let Some(ref gpu) = self.gpu_driver {
            parts.push(gpu.clone());
        }
        parts.join("::")
    }
}
