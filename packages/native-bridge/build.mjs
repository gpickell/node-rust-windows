import { spawnSync } from "child_process";
import { copyFileSync, mkdirSync } from "fs";

function build(target) {
    const child = spawnSync("cargo", ["build", "--release", "--target", target], {
        stdio: "inherit"
    });

    if (!child.status) {
        mkdirSync("dist", { recursive: true });
        copyFileSync(`target/${target}/release/plugin.dll`, `dist/plugin-${target}.node`);
    } else {
        process.exit(child.status);
    }
}

build("x86_64-pc-windows-msvc");
build("i686-pc-windows-msvc");
build("aarch64-pc-windows-msvc");
