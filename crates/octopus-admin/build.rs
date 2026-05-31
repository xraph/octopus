use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=styles/input.css");
    println!("cargo:rerun-if-changed=tailwind.config.js");
    println!("cargo:rerun-if-changed=templates/");

    // Building the admin CSS is best-effort: it needs Node/npm (and network for
    // the first `npm install`), which aren't available in every build
    // environment — e.g. the slim Docker builder image. The output
    // (static/output.css) is a runtime asset, not compiled into the binary, so
    // never fail the crate build over it: warn and skip instead.
    if let Err(err) = build_admin_css() {
        println!(
            "cargo:warning=octopus-admin: skipping Tailwind CSS build ({err}); \
             run `npm install && npm run build:css` in crates/octopus-admin to \
             generate static/output.css"
        );
    }
}

fn build_admin_css() -> Result<(), String> {
    // npm absent → nothing we can do; skip.
    if Command::new("npm").arg("--version").output().is_err() {
        return Err("npm not found".into());
    }

    if !std::path::Path::new("node_modules").exists() {
        println!("Installing npm dependencies...");
        let status = Command::new("npm")
            .arg("install")
            .status()
            .map_err(|e| format!("could not run `npm install`: {e}"))?;
        if !status.success() {
            return Err("`npm install` failed".into());
        }
    }

    println!("Building Tailwind CSS...");
    let status = Command::new("npm")
        .args(["run", "build:css"])
        .status()
        .map_err(|e| format!("could not run `npm run build:css`: {e}"))?;
    if !status.success() {
        return Err("`npm run build:css` failed".into());
    }

    println!("Tailwind CSS built successfully!");
    Ok(())
}
