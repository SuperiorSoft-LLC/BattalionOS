// SPDX-License-Identifier: Apache-2.0
// Paths are relative to the OS root, joined with "/".
export const join = (dir: string, name: string) => (dir ? `${dir}/${name}` : name);
export const parent = (path: string) => path.split("/").slice(0, -1).join("/");
export const crumbs = (dir: string) => (dir ? dir.split("/") : []);
