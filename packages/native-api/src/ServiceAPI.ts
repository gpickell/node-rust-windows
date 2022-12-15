import NodePlugin from "./NodePlugin";

let svc: any;

export class ServiceAPI {
    static create() {
        svc = NodePlugin.setup();
        return new this();
    }

    paused() {
        svc.service_paused();
    }

    running() {
        svc.service_running();
    }

    stopped() {
        svc.service_stopped();
    }

    continuePending() {
        svc.service_continue_pending();
    }

    pausePending() {
        svc.service_pause_pending();
    }

    startPending() {
        svc.service_start_pending();
    }

    stopPending() {
        svc.service_stop_pending();
    }

    simulate(name: string, pauseSupport = false) {
        svc.service_simulate(name, pauseSupport);
    }

    start(name: string, pauseSupport = false) {
        svc.service_start(name, pauseSupport);
    }

    shutdown() {
        svc.service_shutdown();
    }

    clear(handle?: unknown) {
        svc.service_clear(handle);
    }

    watch(callback: (info: string, isStop: boolean) => any): unknown {
        return svc.service_watch(callback);
    }

    post(info: string) {
        svc.service_post(info);
    }
}

export default ServiceAPI;