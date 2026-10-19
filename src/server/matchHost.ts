import type { Host } from "@/types/generated/Host";

/** The inventory host of an incident, matched like the service does (name, alias, short name, IP). */
export function matchHost(hosts: Host[], agentName: string, agentIp: string): Host | undefined {
  const eq = (a: string, b: string) =>
    a.trim().toLowerCase() === b.trim().toLowerCase() && a.trim() !== "";
  const by = (key: string) =>
    hosts.find(
      (h) =>
        eq(h.name, key) ||
        eq(h.address, key) ||
        h.aliases.some((x) => eq(x, key)) ||
        eq(h.name, key.split(".")[0] ?? ""),
    );
  return by(agentName) ?? by(agentIp);
}
