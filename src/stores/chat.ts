import { create } from "zustand";
import type { ChatEvent } from "@/types/generated/ChatEvent";
import type { ChatMessage } from "@/types/generated/ChatMessage";
import type { SessionSummary } from "@/types/generated/SessionSummary";

export interface ToolCall {
  tool: string;
  preview: string | null;
  status: "running" | "ok" | "error";
}

export interface Turn {
  role: "user" | "assistant";
  content: string;
  /** Assistant turn still streaming from Hermes. */
  streaming?: boolean;
  thinking?: string;
  tools?: ToolCall[];
  commentary?: string[];
  error?: string;
}

export interface PendingApproval {
  runId: string;
  command: string | null;
  description: string | null;
  choices: string[];
}

interface ChatState {
  sessions: SessionSummary[];
  activeId: string | null;
  turns: Record<string, Turn[]>;
  /** Run currently streaming per session (null = idle). */
  runs: Record<string, string | null>;
  approvals: Record<string, PendingApproval | null>;
  error: string | null;

  setSessions: (sessions: SessionSummary[]) => void;
  upsertSession: (session: SessionSummary) => void;
  removeSession: (id: string) => void;
  setActive: (id: string | null) => void;
  setHistory: (sessionId: string, messages: ChatMessage[]) => void;
  addUserTurn: (sessionId: string, text: string) => void;
  apply: (sessionId: string, event: ChatEvent) => void;
  clearApproval: (sessionId: string) => void;
  setError: (error: string | null) => void;
}

/** Stored Hermes messages → turns shown in the chat (tool rows are folded away). */
export function turnsFromMessages(messages: ChatMessage[]): Turn[] {
  const out: Turn[] = [];
  for (const m of messages) {
    if (m.role === "user" || m.role === "assistant") {
      if (!m.content.trim()) continue;
      out.push({ role: m.role, content: m.content });
    }
  }
  return out;
}

export const useChatStore = create<ChatState>((set) => ({
  sessions: [],
  activeId: null,
  turns: {},
  runs: {},
  approvals: {},
  error: null,

  setSessions: (sessions) => set({ sessions }),
  upsertSession: (session) =>
    set((s) => ({
      sessions: s.sessions.some((x) => x.id === session.id)
        ? s.sessions.map((x) => (x.id === session.id ? session : x))
        : [session, ...s.sessions],
    })),
  removeSession: (id) =>
    set((s) => {
      const turns = { ...s.turns };
      delete turns[id];
      return { sessions: s.sessions.filter((x) => x.id !== id), turns, activeId: s.activeId === id ? null : s.activeId };
    }),
  setActive: (id) => set({ activeId: id }),
  setHistory: (sessionId, messages) =>
    set((s) => ({ turns: { ...s.turns, [sessionId]: turnsFromMessages(messages) } })),
  addUserTurn: (sessionId, text) =>
    set((s) => ({
      turns: { ...s.turns, [sessionId]: [...(s.turns[sessionId] ?? []), { role: "user", content: text }] },
      error: null,
    })),
  setError: (error) => set({ error }),
  clearApproval: (sessionId) => set((s) => ({ approvals: { ...s.approvals, [sessionId]: null } })),

  apply: (sessionId, event) =>
    set((s) => {
      const list = (s.turns[sessionId] ?? []).slice();
      const last = list[list.length - 1];
      const live = last && last.role === "assistant" && last.streaming ? { ...last } : null;
      const replaceLive = (t: Turn) => {
        if (live) list[list.length - 1] = t;
        else list.push(t);
        return { turns: { ...s.turns, [sessionId]: list } };
      };

      switch (event.kind) {
        case "run_started":
          if (!live) list.push({ role: "assistant", content: "", streaming: true, tools: [] });
          return { turns: { ...s.turns, [sessionId]: list }, runs: { ...s.runs, [sessionId]: event.run_id } };
        case "delta":
          return replaceLive({ ...(live ?? blank()), content: (live?.content ?? "") + event.text });
        case "thinking":
          return replaceLive({ ...(live ?? blank()), thinking: (live?.thinking ?? "") + event.text });
        case "tool_started":
          return replaceLive({
            ...(live ?? blank()),
            tools: [...(live?.tools ?? []), { tool: event.tool, preview: event.preview, status: "running" }],
          });
        case "tool_finished": {
          const tools = (live?.tools ?? []).slice();
          let idx = -1;
          for (let i = tools.length - 1; i >= 0; i--) {
            if (tools[i]?.tool === event.tool && tools[i]?.status === "running") {
              idx = i;
              break;
            }
          }
          const done: ToolCall = { tool: event.tool, preview: event.preview, status: event.ok ? "ok" : "error" };
          if (idx >= 0) tools[idx] = done;
          else tools.push(done);
          return replaceLive({ ...(live ?? blank()), tools });
        }
        case "commentary":
          return replaceLive({ ...(live ?? blank()), commentary: [...(live?.commentary ?? []), event.text] });
        case "approval_request":
          return {
            approvals: {
              ...s.approvals,
              [sessionId]: {
                runId: event.run_id,
                command: event.command,
                description: event.description,
                choices: event.choices,
              },
            },
          };
        case "approval_responded":
          return { approvals: { ...s.approvals, [sessionId]: null } };
        case "completed":
          return replaceLive({ ...(live ?? blank()), content: event.content || live?.content || "", streaming: false });
        case "failed":
          return {
            ...replaceLive({ ...(live ?? blank()), streaming: false, error: event.error }),
            approvals: { ...s.approvals, [sessionId]: null },
          };
        case "cancelled":
          return replaceLive({ ...(live ?? blank()), streaming: false, error: "остановлено" });
        case "ended": {
          const patch = live ? replaceLive({ ...live, streaming: false }) : {};
          return { ...patch, runs: { ...s.runs, [sessionId]: null }, approvals: { ...s.approvals, [sessionId]: null } };
        }
      }
    }),
}));

function blank(): Turn {
  return { role: "assistant", content: "", streaming: true, tools: [] };
}
