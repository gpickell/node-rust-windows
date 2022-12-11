import NodePlugin from "../NodePlugin";

let svc: any;

export class SystemHttpSession {
    readonly ref: unknown;
    readonly name?: string;

    constructor(ref: unknown, name?: string) {
        this.done = this.done.bind(this);
        this.ref = ref;
        this.name = name;
    }

    static create(name: string) {
        svc = NodePlugin.setup();
        svc.http_init(false, true);

        let ref = svc.http_session_create(name);
        return new this(ref, name);
    }

    done() {
        return !this.ref;
    }

    close() {
        this.ref && svc.http_session_close(this.ref);
        Object.assign(this, { ref: undefined });
    }

    listen(url: string) {
        svc.http_session_listen(this.ref, url);
    }
}

export default SystemHttpSession;
