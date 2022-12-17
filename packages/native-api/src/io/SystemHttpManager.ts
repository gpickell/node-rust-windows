import { EventEmitter } from "events";

import RelayHelper, { RelayHelperEvents } from "./RelayHelper";
import SystemHttpRequest from "./SystemHttpRequest";
import SystemHttpSession from "./SystemHttpSession";

export interface On<T>  {
    <K extends keyof RelayHelperEvents>(event: K, listener: (...args: Parameters<RelayHelperEvents[K]>) => any): T;
    (event: string | symbol, listener: (...args: any) => any): T;
}

export interface SystemHttpManager  {
    on: On<this>;
    off: On<this>;

    emit<K extends keyof RelayHelperEvents>(event: K, ...args: Parameters<RelayHelperEvents[K]>): boolean;
    emit(event: string | symbol): boolean;
}

export class SystemHttpManager extends EventEmitter {
    private requests = new Set<RelayHelper>();
    private sessions = new Set<SystemHttpSession>();
    private session?: SystemHttpSession;

    public async process(name: string) {
        await (0 as any);

        const { requests } = this;
        const native = SystemHttpRequest.create(name);
        while (!native.done()) {
            const next = native.clone();
            const helper = new RelayHelper(next);
            requests.add(helper);

            const result = await next.receive();
            if (result === true) {
                helper.relay(this).finally(() => requests.delete(helper));
            } else {
                helper.cancel().finally(() => requests.delete(helper));
            }
        }
    }

    createSession(name: string) {
        const session = this.session = SystemHttpSession.create(name);
        this.sessions.add(session);
    }

    config(...args: (string | string[])[]) {
        if (this.session === undefined) {
            return false;
        }

        this.session.config(...args);
        return true;
    }

    listen(urlPrefix: string) {
        if (this.session === undefined) {
            return false;
        }

        this.session.listen(urlPrefix);
        return true;
    }

    destroy() {
        this.sessions.forEach(x => x.close());
        this.sessions.clear();

        this.requests.forEach(x => x.destroy());
        this.requests.clear();

        this.session = undefined;
    }
}

export default SystemHttpManager;
