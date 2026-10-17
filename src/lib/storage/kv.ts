const databaseName = "filmlab";
const storeName = "kv";

let opened: Promise<IDBDatabase> | undefined;

function database(): Promise<IDBDatabase> {
  opened ??= new Promise((resolve, reject) => {
    const request = indexedDB.open(databaseName, 1);
    request.addEventListener("upgradeneeded", () => request.result.createObjectStore(storeName));
    request.addEventListener("success", () => resolve(request.result));
    request.addEventListener("error", () =>
      reject(request.error ?? new Error("Unable to open browser storage.")),
    );
  });
  return opened;
}

async function transact<T>(
  mode: IDBTransactionMode,
  operate: (store: IDBObjectStore) => IDBRequest<T>,
): Promise<T> {
  const db = await database();
  return new Promise((resolve, reject) => {
    const request = operate(db.transaction(storeName, mode).objectStore(storeName));
    request.addEventListener("success", () => resolve(request.result));
    request.addEventListener("error", () =>
      reject(request.error ?? new Error("Unable to use browser storage.")),
    );
  });
}

export function readValue(key: string): Promise<unknown> {
  return transact<unknown>("readonly", (store) => store.get(key));
}

export async function writeValue(key: string, value: unknown): Promise<void> {
  await transact("readwrite", (store) => store.put(value, key));
}
