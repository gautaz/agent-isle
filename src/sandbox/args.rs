/// A single bubblewrap command-line argument with a fixed arity.
///
/// Each variant carries exactly the operands its option requires, so a
/// malformed invocation (e.g. `--ro-bind` with no destination) is not
/// representable at the type level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BwrapArg {
    Proc,
    Dev,
    Tmpfs(String),
    RoBind { src: String, dst: String },
    Bind { src: String, dst: String },
    SetEnv(String, String),
    Chdir(String),
}

/// Ordered, typed bubblewrap arguments.
///
/// Builders collect typed options and flatten them into a bwrap argv only at
/// the end, guaranteeing every option is emitted with the correct operands.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SandboxArgs(Vec<BwrapArg>);

impl SandboxArgs {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn proc(mut self) -> Self {
        self.0.push(BwrapArg::Proc);
        self
    }

    pub fn dev(mut self) -> Self {
        self.0.push(BwrapArg::Dev);
        self
    }

    pub fn tmpfs(mut self, dst: impl Into<String>) -> Self {
        self.0.push(BwrapArg::Tmpfs(dst.into()));
        self
    }

    pub fn ro_bind(mut self, src: impl Into<String>, dst: impl Into<String>) -> Self {
        self.0.push(BwrapArg::RoBind {
            src: src.into(),
            dst: dst.into(),
        });
        self
    }

    pub fn bind(mut self, src: impl Into<String>, dst: impl Into<String>) -> Self {
        self.0.push(BwrapArg::Bind {
            src: src.into(),
            dst: dst.into(),
        });
        self
    }

    pub fn setenv(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.0.push(BwrapArg::SetEnv(key.into(), value.into()));
        self
    }

    pub fn chdir(mut self, dir: impl Into<String>) -> Self {
        self.0.push(BwrapArg::Chdir(dir.into()));
        self
    }

    /// Flatten into a bwrap argv without the sandboxed program.
    pub fn into_flags(self) -> Vec<String> {
        let mut flags = Vec::new();
        for arg in self.0 {
            match arg {
                BwrapArg::Proc => flags.extend(["--proc".to_string(), "/proc".to_string()]),
                BwrapArg::Dev => flags.extend(["--dev".to_string(), "/dev".to_string()]),
                BwrapArg::Tmpfs(dst) => flags.extend(["--tmpfs".to_string(), dst]),
                BwrapArg::RoBind { src, dst } => flags.extend(["--ro-bind".to_string(), src, dst]),
                BwrapArg::Bind { src, dst } => flags.extend(["--bind".to_string(), src, dst]),
                BwrapArg::SetEnv(key, value) => flags.extend(["--setenv".to_string(), key, value]),
                BwrapArg::Chdir(dir) => flags.extend(["--chdir".to_string(), dir]),
            }
        }
        flags
    }

    /// Flatten and append the program to exec inside the sandbox plus its args.
    pub fn exec(
        self,
        program: impl Into<String>,
        args: impl IntoIterator<Item = String>,
    ) -> Vec<String> {
        let mut flags = self.into_flags();
        flags.push(program.into());
        flags.extend(args);
        flags
    }
}
