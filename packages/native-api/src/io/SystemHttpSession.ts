import NodePlugin from "../NodePlugin";
import SystemHttpRequest from "./SystemHttpRequest";

let svc: any;

export class SystemHttpSession {
    readonly controller: boolean;
    readonly ref: unknown;
    readonly name?: string;

    constructor(ref: unknown, name?: string, controller?: boolean) {
        this.controller = !!controller;
        this.ref = ref;
        this.name = name;
    }

    static create(name?: string) {
        svc = NodePlugin.setup();

        let ref = svc.http_session_create(name);
        return new this(ref, name, name !== undefined);
    }

    static open(name: string) {
        svc = NodePlugin.setup();

        let ref = svc.http_session_open(name);
        return new this(ref, name);
    }

    done() {
        return !!this.ref;
    }

    close() {
        this.ref && svc.http_session_close(this.ref);
        Object.assign(this, { ref: undefined });
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
