import { spawnSync } from "child_process";
import { copyFileSync } from "fs";

const child = spawnSync("cargo", ["build", "--release"], {
    stdio: "inherit"    
});

process.exitCode = child.status;

if (!process.exitCode) {
    copyFileSync("target/release/plugin.dll", "plugin.node");
}
