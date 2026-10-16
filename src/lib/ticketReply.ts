import { api } from "@/services/tauri";
import { useChatStore } from "@/stores/chat";

/** Last non-empty assistant message of the active chat session (the drafted reply). */
export function lastAssistantText(): string | null {
  const chat = useChatStore.getState();
  const id = chat.activeId;
  if (!id) return null;
  const turns = chat.turns[id] ?? [];
  for (let i = turns.length - 1; i >= 0; i--) {
    const t = turns[i];
    if (t && t.role === "assistant" && !t.streaming && t.content.trim() && !t.content.startsWith("⚡")) return t.content.trim();
  }
  return null;
}

/** Send the drafted reply to the pending ticket; returns a message for the user. */
export async function sendDraftReply(): Promise<string> {
  const text = lastAssistantText();
  if (!text) return "Нет черновика: сначала скажите «ответь на тикет <номер>» и дождитесь текста.";
  const pending = await api.ticketPending();
  if (!pending) return "Нет ожидающей заявки: сначала «ответь на тикет <номер>».";
  const id = await api.ticketReply(text);
  return `Ответ отправлен клиенту (заявка ${id}).`;
}
