import { DuplexPair } from "./dist/esm/io/DuplexPair.mjs";


let [a, b] = DuplexPair.create();

b.on("data", data => console.log(data));
a.write("test1");
a.write("test2");

setInterval(() => {}, 300);