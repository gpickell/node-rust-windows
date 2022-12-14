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
    let set = new Set();
    for (const key in responseHeaders) {
        const id = responseHeaders[key];
        max = Math.max(id + 1, max)
        set.add(id);

        responseHeadersByName[key.toLowerCase()] = id;
    }

    for (const key in requestHeaders) {
        const id = requestHeaders[key];
        requestHeadersByIndex[id] = key;

        if (id < max && !set.has(id)) {
            responseHeadersByName[key.toLowerCase()] = id;
        }
    }

    function request(id: number) {
        return requestHeadersByIndex[id] || `X-Header-${id}`;
    }

    function response(name: string) {
        const i = responseHeadersByName[name.toLowerCase()];
        return i !== undefined ? i : -1;
    }

    const verbsByIndex = [
        undefined,
        undefined,
        undefined,
        "OPTIONS", "GET", "HEAD",
        "POST", "PUT", "DELETE",
        "TRACE", "CONNECT", "TRACK",
        "MOVE", "COPY",
        "PROPFIND", "PROPPATCH", "MKCOL",
        "LOCK", "UNLOCK", "SEARCH",
    ];

    const verbsByName = Object.create(null) as Record<string, number>;
    for (const [id, name] of verbsByIndex.entries()) {
        if (typeof name === "string") {
            verbsByName[name] = id;
        }
    }

    function verb(id: number): string | undefined {
        return verbsByIndex[id];
    }

    function method(name: string) {
        return verbsByName[name] || 0;
    }

    return { request, response, verb, method };
}

const mapper = initMapper();
let svc: any;

type BlockItem = Buffer | boolean | number | string | [string, BufferEncoding];

function renderBlock(array: BlockItem[]) {
    let i = 0;
    const data: Buffer[] = [];
    const stage = array.map(x => {
        let enc: BufferEncoding = "utf-8";
        if (Array.isArray(x)) {
            enc = x[1];
            x = x[0];
        }

        if (typeof x === "string") {
            const index = i;
            const part = Buffer.from(x, enc);
            const len = part.byteLength;
            data.push(part);
            data.push(Buffer.alloc(2));
            i += len + 2;

            return [index, len];
        }

        if (Buffer.isBuffer(x)) {
            const index = i;
            const len = x.byteLength;
            data.push(x);
            i += len;

            return [index, len];
        }

        return x as BlockItem;
    });

    const block = data.length == 1 ? data[0] : Buffer.concat(data);
    const result = stage.flat();
    result.unshift(block);

    return result;
}

function addBlockHeader(array: BlockItem[], name: string, value: string) {
    let i = mapper.response(name);
    array.push(i);
    array.push(value);

    if (i < 0) {
        array.push(name);
    }
}

export class Headers extends Array<[string, string, string, boolean]> {
    add(name: string, value: string) {
        value = value.trim();

        if (value) {
            this.push([name.toLowerCase(), name, value, false]);
        }
    }

    all(name: string) {
        name = name.toLowerCase();

        const result: string[] = [];
        for (const [key, _, value, first] of this) {
            if (key === name) {
                if (first) {
                    result.length = 0;
                }
    
                if (value) {
                    result.push(value);
                }
            }
        }

        return result;
    }

    get(name: string) {
        name = name.toLowerCase();

        let result: string | undefined;
        for (const [key, _, value, first] of this) {
            if (key === name) {
                if (first || result === undefined) {
                    result = value || undefined;
                }
            }
        }

        return result;
    }

    set(name: string, value: string) {
        value = value.trim();

        if (value) {
            this.push([name.toLowerCase(), name, value, true]);
        }
    }

    clear() {
        this.length = 0;
    }

    delete(name: string) {
        this.push([name.toLowerCase(), name, "", true]);
    }

    load(hints: string[] | Record<string, string | string[] | number | undefined>) {
        if (Array.isArray(hints)) {
            let key: string | undefined;
            for (const hint of hints) {
                if (key === undefined) {
                    key = hint;
                } else {
                    this.add(key, hint);
                    key = undefined;
                }
            }
        } else {
            for (const key in hints) {
                let hint = hints[key];
                if (typeof hint === "number") {
                    this.add(key, String(hint));
                }

                if (typeof hint === "string") {
                    this.add(key, hint);
                }

                if (Array.isArray(hint)) {
                    hint.forEach(x => this.add(key, x));
                }                
            }
        }
    }

    *render() {
        const groups = new Map<string, [string, string[]]>();
        for (const [key, name, value, first] of this) {
            const group = groups.get(key) || [name, []];
            if (first) {
                group[0] = name;
                group[1].length = 0;
                groups.delete(key);
            }

            if (value) {
                group[1].push(value);
                groups.set(key, group);
            }
        }

        for (const list of groups.values()) {
            yield list;
        }
    }

    *renderFlat() {
        for (const [name, values] of this.render()) {
            for (const value of values) {
                yield [name, value];
            }
        }
    }
}

export class RequestData {
    method = "";
    url = "";
    version = "";

    readonly headers = new Headers();
}

export class ResponseData {
    status = 0;
    reason = "";
    version = "";

    readonly headers = new Headers();
    readonly trailers = new Headers();
}

type Data = string | Buffer | (string | Buffer)[];

function toBuffer(data: string | Buffer) {
    return typeof data === "string" ? Buffer.from(data) : data;
}

export type UserGroup<T = Promise<string>> = [type: string, sid: string, name?: T];

export class SystemHttpRequest {
    readonly id: [unknown];
    readonly ref: [unknown];
    readonly name: string;

    readonly request = new RequestData();
    readonly response = new ResponseData();

    readable = true;
    writable = true;

    chunked = false;
    disconnect = false;
    opaque = false;
    speedy = false;
    user: unknown;

    constructor(ref: [unknown], name: string) {
        this.done = this.done.bind(this);
        this.id = [undefined];
        this.ref = ref;
        this.name = name;
    }

    static create(name: string) {
        svc = NodePlugin.setup();

        const ref = svc.http_request_create(name);
        return new this([ref], name);
    }

    clone() {
        return new SystemHttpRequest(this.ref, this.name);
    }

    done() {
        return !this.ref[0];
    }

    close() {
        const { ref, id } = this;
        id.shift();

        if (ref[0]) {
            svc.http_session_close(ref.pop());
        }
    }

    // @ts-ignore
    push(method: string, url: string, headers: Headers) {
        const path = url.replace(/\?.*/, "");
        const query = url.substring(path.length);
        const block: BlockItem[] = [
            mapper.method(method),
            [path, "ucs-2"],
            query,
        ];

        for (const [name, value] of headers.renderFlat()) {
            addBlockHeader(block, name, value);
        }
        
        svc.http_request_push(this.handle(), this.id[0], ...renderBlock(block));
    }

    handle() {
        const { ref } = this;
        if (ref[0]) {
            return ref[0];
        }

        return undefined;
    }

    ok() {
        this.id.pop();
    }

    async cancel() {
        const { id, ref } = this;
        if (id[0] && ref[0]) {
            return await svc.http_request_cancel(ref[0], id.pop()) as number;
        }

        return 0;
    }

    resolveIdentity(names = false) {
        const { user } = this;
        this.user = undefined;

        if (user) {
            let result = svc.user_groups("viaToken", names, user) as UserGroup[];
            svc.user_close(user);

            return result;
        }

        return [];
    }

    dropIdentity() {
        const { user } = this;
        this.user = undefined;

        if (user) {
            svc.user_close(user);
        }
    }

    async receive(size = 0) {
        const { knownHeaders, unknownHeaders, id, user, ...rest } = await svc.http_request_receive(this.handle(), size);
        if (rest.code !== 0) {
            return rest.code as number;
        }

        this.id[0] = id;

        const { request, response } = this;
        request.method = rest.customVerb || mapper.verb(rest.verb) || "";
        request.url = rest.url || "";
        request.version = rest.version;
        response.version = rest.version;
        this.speedy = !!rest.http2;

        for (const [i, value] of knownHeaders.entries()) {
            value && request.headers.add(mapper.request(i), value);
        }

        request.headers.load(unknownHeaders);

        this.readable = !!rest.body;
        this.user = user;
        return true;
    }

    async receiveData(size = 0) {
        const data = Buffer.alloc(size > 0 ? size : 4096)
        const result = await svc.http_request_receive_data(this.handle(), this.id[0], data);
        if (result.eof) {
            this.readable = false;
            return undefined;
        }

        if (result.code) {
            return result.code as number;
        }

        return data.subarray(0, result.size);
    }

    // @ts-ignore
    async send(final = false) {
        if (final) {
            this.writable = false;
        }

        const { response } = this;
        const [major, minor] = response.version.split(".");
        const block: BlockItem[] = [
            this.opaque,
            this.writable,
            !this.writable && this.disconnect,
            response.status,
            Number(major), Number(minor),
            response.reason,
        ];

        let te = response.headers.get("Transfer-Encoding") || "";
        this.chunked = te === "chunked";

        for (const [name, value] of response.headers.renderFlat()) {
            addBlockHeader(block, name, value);
        }

        if (!this.writable) {
            for (const [name, value] of response.trailers.renderFlat()) {
                addBlockHeader(block, name, value);
            }
        }

        // console.log(renderBlock(block));
        const { code } = await svc.http_request_send(this.handle(), this.id[0], ...renderBlock(block));
        return code as number;
    }

    async sendData(data: Data, final = false) {
        if (!Array.isArray(data)) {
            data = [data];
        }

        if (final) {
            this.writable = false;
        }

        const block: BlockItem[] = [
            this.opaque,
            this.writable,
            !this.writable && this.disconnect,
        ];

        let hasTrailers = false;
        if (!this.writable && this.chunked) {
            const { response } = this;
            for (const [name, value] of response.trailers.renderFlat()) {
                hasTrailers = true;
                block.push(name, value);
            }
        }

        let chunks = data.map(toBuffer);
        chunks = chunks.filter(x => x.byteLength > 0);

        if (this.chunked) {
            const array = chunks.map(x => {
                const len = x.byteLength.toString(16);
                return [`${len}\r\n`, x, "\r\n"];
            });

            if (!this.writable) {
                array.push(["0\r\n"]);

                if (!hasTrailers) {
                    array.push(["\r\n"]);
                }
            }
            
            chunks = array.flat().map(toBuffer);
        }

        // console.log(renderBlock(block));
        const { code } = await svc.http_request_send_data(this.handle(), this.id[0], chunks.length, ...chunks, ...renderBlock(block));
        return code as number;
    }
}

export default SystemHttpRequest;
