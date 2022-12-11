import { isAbsolute } from "path";
import { defineConfig } from "rollup";

import typescript from "@rollup/plugin-typescript";

const inputs = [
    "NodePlugin",
    "io/SystemHttpManager",
    "io/SystemHttpRequest",
    "io/SystemHttpSession",
];

export default defineConfig({
    input: Object.fromEntries(inputs.map(x => [x, `src/${x}.ts`])),

    external(id, importer) {
        if (!isAbsolute(id) && id[0] !== ".") {
            return true;
        }
    },

    output: [
        {
            dir: "dist",
            format: "cjs",
            entryFileNames: "cjs/[name].cjs",
            chunkFileNames: "assets/asset.[hash].cjs",
            exports: "named",
            sourcemap: true,
        },
        {
            dir: "dist",
            format: "esm",
            entryFileNames: "esm/[name].mjs",
            chunkFileNames: "assets/asset.[hash].mjs",
            sourcemap: true,
        },
    ],

    plugins: [
        typescript()
    ]
});
