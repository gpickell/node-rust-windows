import NodePlugin from "../NodePlugin";
import SystemHttpRequest from "./SystemHttpRequest";

let svc: any;

export class SystemHttpSession {
    readonly ref: unknown;
    readonly name?: string;

    constructor(ref: unknown, name?: string) {
        this.ref = ref;
        this.name = name;
    }

    static create(name?: string) {
        svc = NodePlugin.setup();

        let ref = svc.http_session_create(name);
        return new this(ref, name);
    }

    static open(name: string) {
        svc = NodePlugin.setup();

        let ref = svc.http_session_open(name);
        return new this(ref, name);
    }

    close() {
        this.ref && svc.http_session_close(this.ref);
        Object.assign(this, { ref: undefined });
    }

    isController() {
        return svc.http_session_is_controller(this.ref);
    }

    listen(url: string) {
        svc.http_session_listen(this.ref, url);
    }

    request() {
        let ref = svc.http_session_request(this.ref);
        return new SystemHttpRequest(ref);
    }
}

export default SystemHttpSession;
