import { UserGroup } from "../UserAPI";
import Headers from "./Headers";

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

export type Data = string | Buffer | (string | Buffer)[];

export interface Request {
    readonly request: RequestData;
    readonly response: ResponseData;

    opaque: boolean;
    disconnect: boolean;

    done(): boolean;
    
    cancel(): Promise<number>;
    close(): void;
    ok(): void;

    receive(size?: number): Promise<number | true>;
    receiveData(size?: number): Promise<Buffer | number | undefined>;
    send(final?: boolean): Promise<number>;
    sendData(data: Data, final?: boolean): Promise<number>;

    push(method: string, url: string, headers: Headers): void;

    dropIdentity(): void;
    resolveIdentity(names?: boolean): UserGroup[];
}

export default Request;