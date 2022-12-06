import NodePlugin from "../NodePlugin";

function initMapper() {
    let requestHeaders = Object.assign(Object.create(null) as Record<string, number>, {
        "Cache-Control": 0,
        "Connection": 1,
        "Date": 2,
        "Keep-Alive": 3,
        "Pragma": 4,
        "Trailer": 5,
        "Transfer-Encoding": 6,
        "Upgrade": 7,
        "Via": 8,
        "Warning": 9,
        "Allow": 10,
        "Content-Length": 11,
        "Content-Type": 12,
        "Content-Encoding": 13,
        "Content-Language": 14,
        "Content-Location": 15,
        "Content-MD5": 16,
        "Content-Range": 17,
        "Expires": 18,
        "Last-Modified": 19,
        "Accept": 20,
        "Accept-CharSet": 21,
        "Accept-Encoding": 22,
        "Accept-Language": 23,
        "Authorization": 24,
        "Cookie": 25,
        "Expect": 26,
        "From": 27,
        "Host": 28,
        "If-Match": 29,
        "If-Modified-Since": 30,
        "If-None-Match": 31,
        "If-Range": 32,
        "If-Unmodified-Since": 33,
        "Max-Forwards": 34,
        "Proxy-Authorization": 35,
        "Referer": 36,
        "Range": 37,
        "Te": 38,
        "Translate": 39,
        "User-Agent": 40,
    });

    let responseHeaders = Object.assign(Object.create(null) as Record<string, number>, {
        "Accept-Ranges": 20,
        "Age": 21,
        "Etag": 22,
        "Location": 23,
        "Proxy-Authenticate": 24,
        "Retry-After": 25,
        "Server": 26,
        "Set-Cookie": 27,
        "Vary": 28,
        "WWW-Authenticate": 29
    });

    let requestHeadersByIndex = [] as string[];
    let responseHeadersByName = Object.create(null) as Record<string, number>;

    let max = 0;
    for (const key in responseHeaders) {
        const id = requestHeaders[key];
        max = Math.max(id, max)

        responseHeadersByName[key.toLowerCase()] = responseHeaders[key];
    }

    for (const key in requestHeaders) {
        const id = requestHeaders[key];
        requestHeadersByIndex[id] = key;

        if (id < max && responseHeadersByName[id] === undefined) {
            responseHeadersByName[key.toLowerCase()] = id;
        }
    }

    function request(id: number) {
        return requestHeadersByIndex[id];
    }

    function response(name: string) {
        return responseHeadersByName[name.toLowerCase()];
    }

    return { request, response };
}

const mapper = initMapper();
let svc: any;

export interface RequestData {
    verb: string;
    url: string;
    version: string;
    headers: Record<string, string>;
}

export class SystemHttpRequest {
    readonly id: unknown;
    readonly ref: unknown;

    constructor(ref: unknown) {
        svc = NodePlugin.setup();
        this.ref = ref;
    }

    close() {
        this.ref && svc.http_request_close(this.ref);
        Object.assign(this, { ref: undefined })
    }

    flags(more?: boolean, opaque?: boolean) {
        svc.http_request_flags(this.ref, more, opaque);
    }

    // @ts-ignore
    push(verb: string, url: string, headers: Record<string, string>) {

    }

    async cancel() {
        return svc.http_request_receive(this.ref, this.id);
    }

    async receive(size?: number) {
        const { knownHeaders, unknownHeaders, id, ...rest } = await svc.http_request_receive(this.ref, size);
        if (rest.code !== 0) {
            return rest;
        }

        const headers = Object.create(null);
        if (knownHeaders && unknownHeaders) {
            for (const [i, value] of knownHeaders.entries()) {
                if (value) {
                    headers[mapper.request(i)] = value;
                }
            }

            Object.assign(headers, unknownHeaders);
        }

        Object.assign(this, { id });
        return Object.assign(rest, { headers });
    }

    async receiveData(size?: number) {
        return svc.http_request_receive_data(this.ref, this.id, size);
    }

    // @ts-ignore
    async send(version: string, status: string, reason: string, headers: Record<string, string>) {

    }

    // @ts-ignore
    async sendData(data: string, trailers: Record<string, string>) {

    }
}

export default SystemHttpRequest;
