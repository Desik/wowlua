//! `wowlua debug` (spec debugger §18.12.1): the Debug Adapter Protocol on
//! stdio or a TCP port, sessions built from the package's test build.

use std::path::PathBuf;
use std::sync::Arc;

use wowlua_debug::dap::{CommandOutput, PackageCommand};

use crate::GlobalArgs;
use crate::output::Output;

struct CliLauncher {
    global: GlobalArgs,
    root: PathBuf,
}

impl wowlua_debug::dap::Launcher for CliLauncher {
    fn root(&self) -> PathBuf {
        self.root.clone()
    }

    fn run(
        &self,
        opts: &wowlua_debug::session::Options,
        session: &mut dyn FnMut(
            &wowlua_test::Suite,
            Option<wowlua_debug::controller::Screenshotter>,
        ),
    ) -> Result<(), String> {
        let out = Output::new(&self.global);
        let instrument: &(dyn Fn(&mut wowlua_vm::Vm) + Sync) = &wowlua_debug::session::instrument;
        crate::commands::with_suite(
            &out,
            &self.global,
            opts.flavor.as_deref(),
            Some(instrument),
            &mut |suite| {
                // The launch's world profile over the manifest's.
                if let Some(p) = &opts.profile {
                    suite.profile = Some(p.clone());
                }
                session(suite, crate::screenshot::screenshotter(suite))
            },
        )
    }

    /// The command as wowlua itself runs it, in the package; a test run's
    /// reports in a folder of their own, read back.
    fn command(&self, command: &PackageCommand) -> Result<CommandOutput, String> {
        // (for an agent: one line a problem, no colours)
        let quiet = ["--message-format", "short", "--color", "never"].map(String::from);
        let flavor = |f: &Option<String>| -> Vec<String> {
            f.iter()
                .flat_map(|f| ["--flavor".to_string(), f.clone()])
                .collect()
        };
        let mut reports = None;
        let args: Vec<String> = match command {
            PackageCommand::Check { flavor: f } => {
                [vec!["check".into()], flavor(f), quiet.to_vec()].concat()
            }
            PackageCommand::Fmt { check, paths } => {
                let mut a = vec!["fmt".to_string()];
                if *check {
                    a.push("--check".into());
                }
                a.extend(paths.iter().cloned());
                a
            }
            PackageCommand::Build { release, flavor: f } => {
                let mut a = vec!["build".to_string()];
                if *release {
                    a.push("--release".into());
                }
                [a, flavor(f), quiet.to_vec()].concat()
            }
            PackageCommand::Test {
                filter,
                flavor: f,
                coverage,
            } => {
                static RUN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                let dir = std::env::temp_dir().join(format!(
                    "wowlua-mcp-test-{}-{}",
                    std::process::id(),
                    RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                ));
                let mut a = vec!["test".to_string()];
                a.extend(filter.iter().cloned());
                a.extend(flavor(f));
                if *coverage {
                    a.push("--coverage".into());
                }
                a.push("--reporter".into());
                a.push(
                    if *coverage {
                        "json,coverage-json"
                    } else {
                        "json"
                    }
                    .into(),
                );
                a.push("--output".into());
                a.push(dir.to_string_lossy().into_owned());
                reports = Some(dir);
                [a, quiet.to_vec()].concat()
            }
        };
        let (ok, text) = self.run_wowlua(&args)?;
        let mut out = CommandOutput {
            ok,
            text,
            ..CommandOutput::default()
        };
        if let Some(dir) = reports {
            let read = |name: &str| {
                std::fs::read(dir.join(name))
                    .ok()
                    .and_then(|b| serde_json::from_slice(&b).ok())
            };
            out.results = read("results.json");
            out.coverage = read("coverage.json");
            let _ = std::fs::remove_dir_all(&dir);
        }
        Ok(out)
    }
}

impl CliLauncher {
    /// `wowlua` itself with `args`, in the package: whether it succeeded,
    /// and what it printed.
    fn run_wowlua(&self, args: &[String]) -> Result<(bool, String), String> {
        let exe = std::env::current_exe().map_err(|e| format!("wowlua's own path: {e}"))?;
        let mut cmd = std::process::Command::new(exe);
        if let Some(m) = &self.global.manifest {
            cmd.arg("--manifest").arg(m);
        }
        if let Some(p) = &self.global.package {
            cmd.arg("--package").arg(p);
        }
        let out = cmd
            .args(args)
            .current_dir(&self.root)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("running wowlua {}: {e}", args.join(" ")))?;
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        Ok((out.status.success(), text))
    }
}

/// Serves MCP on stdio until the client disconnects.
pub fn mcp(out: &Output, global: &GlobalArgs) -> bool {
    let Some(root) = crate::commands::package_root(out, global) else {
        eprintln!("error: wowlua mcp runs in an addon package that loads");
        return false;
    };
    let launcher = Arc::new(CliLauncher {
        global: global.clone(),
        root,
    });
    let stdin = std::io::stdin();
    wowlua_debug::mcp::Server::new(launcher).serve(stdin.lock(), std::io::stdout());
    true
}

/// Serves DAP until the client disconnects.
pub fn debug(out: &Output, global: &GlobalArgs, port: Option<u16>) -> bool {
    let Some(root) = crate::commands::package_root(out, global) else {
        eprintln!("error: wowlua debug runs in an addon package that loads");
        return false;
    };
    let launcher = Arc::new(CliLauncher {
        global: global.clone(),
        root,
    });
    match port {
        None => {
            wowlua_debug::dap::serve(std::io::stdin(), Box::new(std::io::stdout()), launcher);
            true
        }
        Some(port) => {
            let listener = match std::net::TcpListener::bind(("127.0.0.1", port)) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("error: cannot listen on port {port}: {e}");
                    return false;
                }
            };
            out.status("Listening", format!("for a DAP client on 127.0.0.1:{port}"));
            match listener.accept() {
                Ok((stream, _)) => {
                    let Ok(reader) = stream.try_clone() else {
                        return false;
                    };
                    wowlua_debug::dap::serve(reader, Box::new(stream), launcher);
                    true
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    false
                }
            }
        }
    }
}
