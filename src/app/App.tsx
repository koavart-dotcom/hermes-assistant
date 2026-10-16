import { Navigate, Route, Routes } from "react-router";
import { Layout } from "@/app/Layout";
import { ChatPage } from "@/features/chat/ChatPage";
import { ApprovalsPage } from "@/features/approvals/ApprovalsPage";
import { VoicePage } from "@/features/voice/VoicePage";
import { StatusPage } from "@/features/status/StatusPage";
import { SettingsPage } from "@/features/settings/SettingsPage";
import { SetupPage } from "@/features/setup/SetupPage";
import { TicketsPage } from "@/features/tickets/TicketsPage";

export function App() {
  return (
    <Routes>
      {/* Separate always-on-top window, no sidebar. */}
      <Route path="/tickets" element={<TicketsPage />} />
      <Route element={<Layout />}>
        <Route index element={<Navigate to="/chat" replace />} />
        <Route path="/chat" element={<ChatPage />} />
        <Route path="/approvals" element={<ApprovalsPage />} />
        <Route path="/voice" element={<VoicePage />} />
        <Route path="/status" element={<StatusPage />} />
        <Route path="/settings" element={<SettingsPage />} />
        <Route path="/setup" element={<SetupPage />} />
        <Route path="*" element={<Navigate to="/chat" replace />} />
      </Route>
    </Routes>
  );
}
