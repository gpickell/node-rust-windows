/*
    This facility is meant to provide an opaque way to interact with an underlying
    push system of some kind. This should not rely on any other facility so that:

    1. Testing is viable without accessing a windows system.
    2. Abstraction layers are viable for building cross-platform solutions.  
*/

import Headers from "./Headers";

export type PushItem = [method: string, url: string, headers: Headers];

export class PushAPI extends Array<PushItem> {
    get(url: string, ...headers: string[]) {
        const result = new Headers();
        result.load(headers);

        this.push(["GET", url, result]);

        return result;
    }
}

export default PushAPI;
