import { Duplex } from "stream";

function done() {}

function makeEarlyError() {
    return new TypeError("DuplexPair destroyed early.");
}

interface DestroyCallback {
    (error: Error | null): void;
}

interface FetchCallback {
    (error?: Error | null | undefined): void;
}

export class DuplexPair extends Duplex {
    private _fetch?: FetchCallback;
    private _other?: DuplexPair;

    private constructor(other?: DuplexPair) {
        super({
            autoDestroy: true,
        });

        this._other = other || new DuplexPair(this);
    }

    _read() {
        const { _fetch } = this;
        if (_fetch && _fetch !== done) {
            setImmediate(_fetch);
            this._fetch = undefined;
        }
    }

    _write(chunk: any, encoding: BufferEncoding, callback: FetchCallback) {
        const { _other } = this;
        if (_other !== undefined) {
            let data = chunk;
            if (typeof data === "string") {
                data = Buffer.from(data, encoding);
            }
    
            _other.push(data);
            _other._fetch = callback;
        }
    }

    _writev(chunks: { chunk: any, encoding: BufferEncoding } [], callback: FetchCallback) {
        const { _other } = this;
        if (_other !== undefined) {
            let last = chunks.pop();
            for (const { chunk, encoding } of chunks) {
                let data = chunk;
                if (typeof data === "string") {
                    data = Buffer.from(data, encoding);
                }
    
                _other.push(data);
            }
    
            if (last) {
                const { chunk, encoding } = last
                let data = chunk;
                if (typeof data === "string") {
                    data = Buffer.from(data, encoding);
                }

                _other.push(data);
                _other._fetch = callback;    
            } else {
                setImmediate(callback);
            }
        }
    }

    _final(callback: FetchCallback): void {
        const { _other } = this;
        if (_other !== undefined) {
            setImmediate(callback);
            _other._fetch = done;
            _other.push(null);
        }
    }

    _destroy(error: Error | null, callback: DestroyCallback)  {
        setImmediate(callback, error);

        const { _fetch, _other } = this;
        this._fetch = undefined;
        this._other = undefined;

        if (_other) {
            _other._other = undefined;

            if (_fetch !== done || _other._fetch !== done) {
                _other.destroy();
            }
        }

        if (_fetch && _fetch !== done) {
            setImmediate(_fetch, makeEarlyError());
        }
    }

    static create(): [DuplexPair, DuplexPair] {
        const a = new this();
        return [a, a._other!];
    }
}

export default DuplexPair;
