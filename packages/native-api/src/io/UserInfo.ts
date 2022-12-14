import { UserGroup } from "./SystemHttpRequest";
export { UserGroup };

let map = new WeakMap<any, UserGroup[]>();

class UserInfo {
    static find<T>(hint: T) {
        return map.get(hint) || [];        
    }

    static async resolve<T>(hint: T) {
        const groups = this.find(hint);
        const result = await Promise.all(groups.map(async x => {
            return await Promise.all(x) as UserGroup<string>;
        }));

        return result;
    }

    static register<T>(hint: T, groups: UserGroup[]) {
        map.set(hint, groups);
    }
}

export default UserInfo;
