use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=styles/input.css");
    println!("cargo:rerun-if-changed=tailwind.config.js");
    println!("cargo:rerun-if-changed=templates/");

    // Check if npm is available
    let npm_check = Command::new("npm").arg("--version").output();

    if npm_check.is_err() {
        eprintln!("Warning: npm not found. Skipping CSS build.");
        eprintln!("Install Node.js and run 'npm install && npm run build:css' manually.");
        return;
    }

    // Check if node_modules exists, if not run npm install
    let node_modules = std::path::Path::new("node_modules");
    if !node_modules.exists() {
        println!("Installing npm dependencies...");
        let status = Command::new("npm")
            .arg("install")
            .status()
            .expect("Failed to run npm install");

        if !status.success() {
            panic!("npm install failed");
        }
    }

    // Build CSS with Tailwind
    println!("Building Tailwind CSS...");
    let status = Command::new("npm")
        .arg("run")
        .arg("build:css")
        .status()
        .expect("Failed to build CSS");

    if !status.success() {
        panic!("CSS build failed");
    }

    println!("Tailwind CSS built successfully!");
}
