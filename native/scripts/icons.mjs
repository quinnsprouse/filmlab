import { writeFileSync } from "node:fs";

import * as icons from "@hugeicons/core-free-icons";

const library = new Map(Object.entries(icons));
const wanted = process.argv.slice(2);
const attrs = (o) =>
  Object.entries(o)
    .filter(([k]) => k !== "key")
    .map(([k, v]) => `${k.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`)}="${v}"`)
    .join(" ");
for (const name of wanted) {
  const icon = library.get(name);
  if (!icon) {
    console.error(`missing ${name}`);
    process.exitCode = 1;
    continue;
  }
  const body = icon
    .map(([tag, a]) => `<${tag} ${attrs({ ...a, strokeWidth: a.strokeWidth ?? "1.5" })}/>`)
    .join("");
  const file = name
    .replace(/Icon$/, "")
    .replace(/([a-z0-9])([A-Z])/g, "$1-$2")
    .toLowerCase();
  writeFileSync(
    `native/assets/icons/${file}.svg`,
    `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round">${body}</svg>\n`,
  );
}
