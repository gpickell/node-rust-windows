import { createRequire } from "module";
import { fileURLToPath } from "url";

let svc: any;

const ENV_HINT = "NODE_RUST_WINDOWS_PLUGIN_PATH";

export namespace NodePlugin {
    export function setup(path?: string, hint?: string) {
        if (svc !== undefined) {
            return svc;
        }

        if (path === undefined) {
            path = fileURLToPath(import.meta.url);
        }

        if (path.startsWith("file:///")) {
            path = fileURLToPath(path);
        }

        const r = createRequire(path);
        hint = hint || process.env[ENV_HINT] || "@tsereact/node-rust-windows-native-bridge/plugin.node";
        hint = r.resolve(hint);
        svc = r(hint);

        if (process.env[ENV_HINT] === undefined) {
            process.env[ENV_HINT] = hint;
        }
    }
}

export default NodePlugin;
