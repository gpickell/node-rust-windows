import { resolve } from "path";
import { pathToFileURL, fileURLToPath } from "url";
import { spawnSync } from "child_process";

const base = fileURLToPath(import.meta.url);

const args = [
    process.execPath,
    "-r", resolve(base, "../../.pnp.cjs"),
    "--loader", pathToFileURL(resolve(base, "../../.pnp.loader.mjs")).toString(),
    resolve(base, "../test.mjs")
];

console.log(args);

/*
spawnSync(args.shift(), args, {
    stdio: "inherit",
    shell: false
});
*/

function encode(arg) {
    if (arg.indexOf(" ") < 0) {
        return arg;
    }

    return `"${arg}"`;
}

const sc_args = [
    "create", "my_node_service",
    "displayname=", "My Node Service",
    "obj=", "NT SERVICE\\my_node_service",
    "binpath=", args.map(encode).join(" "),
];

spawnSync("sc.exe", sc_args, {
    stdio: "inherit",
    shell: false
});