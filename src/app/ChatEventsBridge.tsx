import { useEffect } from "react";
import { isTauri, onChatEvent } from "@/services/tauri";
import { useChatStore } from "@/stores/chat";

/**
 * Feeds every `chat:event` into the chat store for the whole app lifetime,
 * so approvals, streaming turns and run state stay correct on any page
 * (the Approvals page used to show stale cards when Chat was not mounted).
 */
export function ChatEventsBridge() {
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    const apply = useChatStore.getState().apply;
    onChatEvent((env) => apply(env.session_id, env.event)).then((u) => (unlisten = u));
    return () => unlisten?.();
  }, []);
  return null;
}
