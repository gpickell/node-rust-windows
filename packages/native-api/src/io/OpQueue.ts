import { Readable, Writable } from "stream";

import Request from "./Request";

export class OpQueue extends Set<() => boolean | Promise<boolean>> {
    private done: () => boolean;
    private resolve: (value: boolean) => void;
    private working = false;

    private async loop() {
        if (this.working) {
            return;
        }

        this.working = true;
        await (0 as any);

        for (const action of this) {
            if (this.done()) {
                break;
            }

            const promise = new Promise<boolean>(x => x(action()));
            promise.then(x => x && this.fail(), () => this.fail());
        }

        this.clear();
        this.working = false;
    }

    constructor(done: () => boolean, resolve: (value: boolean) => void) {
        super();
        this.done = done;
        this.resolve = resolve;

        this.good = this.good.bind(this);
        this.fail = this.fail.bind(this);
    }

    push(fn: () => boolean | Promise<boolean>) {
        if (!this.done()) {
            this.add(fn);

            if (!this.working) {
                this.loop();
            }
        }
    }

    good() {
        this.push(() => {
            this.clear();
            this.done = () => true;
            this.resolve(false);

            return false;
        });
    }

    fail() {
        this.clear();
        this.done = () => true;
        this.resolve(true);
    }

    receive(from: Request, to: Writable) {
        const relay = async () => {
            const data = await from.receiveData();
            if (Buffer.isBuffer(data)) {
                if (to.write(data)) {
                    this.push(relay);
                }

                return false;
            }

            if (data === undefined) {
                to.end();
                this.good();

                return false;
            }

            return true;
        };

        to.on("drain", () => this.push(relay));
        to.on("error", this.fail);
        to.on("close", this.good);

        this.push(relay);
    }

    send(from: Readable, to: Request, end?: () => void) {
        from.on("data", data => {
            from.pause();
            this.push(async () => {
                if (await to.sendData(data) != 0) {
                    return true;
                }

                from.resume();
                return false;
            });
        });

        from.on("end", async () => {
            this.push(async () => {
                end?.();

                if (await to.sendData([], true) != 0) {
                    return true;
                }

                this.good();
                return false;
            });
        });

        from.on("error", this.fail);
        from.on("close", this.good);

        this.push(async () => {
            const result = await to.send();
            if (result !== 0) {
                return true;
            }

            from.resume();
            return false;
        });
    }
}

export default OpQueue;
