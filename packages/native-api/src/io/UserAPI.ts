/*
    This facility is meant to provide an opaque way to interact with an underlying
    identity of some kind. This should not rely on any other facility so that:

    1. Testing is viable without accessing a windows system.
    2. Abstraction layers are viable for building cross-platform solutions.  
*/

const map = new WeakMap<any, () => Promise<UserAPI>>();
const patches = new WeakMap<any, (hint: any) => Promise<UserAPI>>();

export type Resolve<T extends boolean> = T extends true ? string : Promise<string>;
export type UserGroup<T extends boolean = boolean> = [type: string, sid: string, name?: Resolve<T>];

export class UserAPI<T extends boolean = boolean> extends Array<UserGroup<T>> {
    findAll(type: string) {
        const result = [] as [string?, Resolve<T>?][];
        for (const [_, id, value] of this) {
            if (_ === type) {
                result.push([id, value]);
            }
        }

        return result;
    }

    findOne(type: string): [string?, Resolve<T>?] {
        for (const [_, id, value] of this) {
            if (_ === type) {
                return [id, value];
            }
        }

        return [];
    }

    async resolve() {
        for (const group of this) {
            group[2] = await group[2];
        }
        
        return this as any as Promise<UserAPI<true>>;
    }

    static async find(hint: any) {
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

        const result = new this();
        map.set(hint, () => Promise.resolve(result));

        return result;
    }

    static async resolve(hint: any) {
        const api = await this.find(hint);
        return await api.resolve();
    }

    static patch<T>(hint: T, resolver: (hint: T) => Promise<UserGroup[]>) {
        patches.set(hint, async (hint: T) => {
            const execute = async () => new UserAPI(...await resolver(hint));
            const result = execute();
            map.set(hint, () => result);

            return result;
        });
    }

    static register(hint: any, resolver: () => Promise<UserGroup[]>) {
        map.set(hint, async () => {
            const execute = async () => new UserAPI(...await resolver());
            const result = execute();
            map.set(hint, () => result);

            return await result;
        });
    }
}

export default UserAPI;
