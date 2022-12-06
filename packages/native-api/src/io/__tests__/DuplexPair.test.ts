import { DuplexPair } from "../DuplexPair";

describe("DuplexPair", () => {
    test("auto close", async () => {
        const [a, b] = DuplexPair.create();

        let end = Promise.all([
            new Promise(x => a.once("close", x)),
            new Promise(x => b.once("close", x)),
        ]);

        a.write("test1");
        a.write("test2");
        a.end();
        b.end();

        a.resume();
        b.resume();

        await end;
    });

    test("demand close", async () => {
        const [a, b] = DuplexPair.create();

        let end = Promise.all([
            new Promise(x => a.once("close", x)),
            new Promise(x => b.once("close", x)),
        ]);

        a.destroy();
        await end;
    });
});
