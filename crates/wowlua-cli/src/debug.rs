//! `wowlua debug` (spec debugger §18.12.1): the Debug Adapter Protocol on
//! stdio or a TCP port, sessions built from the package's test build.

use std::path::PathBuf;
use std::sync::Arc;

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

    /// The command as wowlua itself runs it, in the package.
    fn command(&self, args: &[String]) -> Result<(bool, String), String> {
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
