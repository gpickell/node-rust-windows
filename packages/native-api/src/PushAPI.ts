/*
    This facility is meant to provide an opaque way to interact with an underlying
    push system of some kind. This should not rely on any other facility so that:

    1. Testing is viable without accessing a windows system.
    2. Abstraction layers are viable for building cross-platform solutions.  
*/

import Headers from "./io/Headers";

const map = new WeakMap<any, () => PushAPI>();
const patches = new WeakMap<any, (hint: any) => PushAPI>();

export type PushItem = [method: string, url: string, headers: Headers];

export class PushAPI extends Array<PushItem> {
    get(url: string, ...headers: string[]) {
        const result = new Headers();
        result.load(headers);

        this.push(["GET", url, result]);

        return result;
    }

    static find(hint: any) {
        if (typeof hint !== "object" || hint === null) {
            throw new TypeError("Must be given an object.");
        }

        const first = map.get(hint);
        if (first) {
            return first();
        }

        let proto = hint;
        while (proto = Object.getPrototypeOf(proto)) {
            const next = patches.get(proto);
            if (next) {
                return next(hint);
            }
        }

        const result = new PushAPI();
        map.set(hint, () => result);

        return result;
    }

    static patch<T>(hint: T, resolver: (hint: T) => PushAPI) {
        patches.set(hint, (hint: T) => {
            const result = resolver(hint);
            map.set(hint, () => result);

            return result;
        });
    }

    static register(hint: any, resolver: () => PushAPI) {
        map.set(hint, () => {
            const result = resolver();
            map.set(hint, () => result);

            return result;
        });
    }
}

export default PushAPI;
