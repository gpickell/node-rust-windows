import NodePlugin from "../NodePlugin";

let svc: any;

export class SystemHttpSession {
    readonly ref: [unknown];
    readonly name?: string;

    constructor(ref: [unknown], name?: string) {
        this.ref = ref;
        this.name = name;
    }

    static create(name: string) {
        svc = NodePlugin.setup();

        let ref = svc.http_session_create(name);
        return new this([ref], name);
    }

    config(...args: (string | string[])[]) {
        svc.http_session_config(this.handle(), ...args.flat());
    }

    handle() {
        const { ref } = this;
        if (ref[0]) {
            return ref[0];
        }

        return undefined;
    }

    close() {
        const { ref } = this;
        if (ref[0]) {
            svc.http_session_close(ref.pop());
        }
    }

    listen(url: string) {
        svc.http_session_listen(this.handle(), url);
    }

    release(url: string) {
        svc.http_session_release(this.handle(), url);
    }
}

export default SystemHttpSession;
