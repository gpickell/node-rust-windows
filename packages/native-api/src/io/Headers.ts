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

export default Headers;