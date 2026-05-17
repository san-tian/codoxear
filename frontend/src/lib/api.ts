import type {
  ChangedFilesResponse,
  CodexConfigResponse,
  CwdSuggestionsResponse,
  DiagnosticsResponse,
  EditSessionResponse,
  FileReadResponse,
  FileSearchResponse,
  HarnessConfig,
  HistoryResponse,
  LiveResponse,
  MeResponse,
  NotificationSubscriptionsResponse,
  QueueResponse,
  RestartServiceResponse,
  ResumeCandidatesResponse,
  ShareCreateResponse,
  ShareFilesResponse,
  ShareListResponse,
  ShareLoginResponse,
  ShareMessageResponse,
  ShareSet,
  SessionsResponse,
  TailResponse,
  VersionStatusResponse,
  VoiceSettingsResponse,
} from "./types";

const API_BASE = (import.meta.env.VITE_CODOXEAR_API_BASE || "").replace(/\/$/, "");

async function readJson<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`, {
    credentials: "same-origin",
    ...init,
    headers: {
      ...(init?.body ? { "Content-Type": "application/json" } : {}),
      ...(init?.headers || {}),
    },
  });
  let payload: unknown = null;
  const contentType = response.headers.get("content-type") || "";
  if (contentType.includes("application/json")) {
    payload = await response.json();
  }
  if (!response.ok) {
    const message =
      payload && typeof payload === "object" && "error" in payload && typeof payload.error === "string"
        ? payload.error
        : `${response.status} ${response.statusText}`;
    throw Object.assign(new Error(message), { status: response.status, payload });
  }
  return payload as T;
}

export const api = {
  me(): Promise<MeResponse> {
    return readJson<MeResponse>("/api/me");
  },
  fetchVersionStatus(): Promise<VersionStatusResponse> {
    return readJson<VersionStatusResponse>("/api/version_status");
  },
  login(password: string): Promise<{ ok: true }> {
    return readJson<{ ok: true }>("/api/login", {
      method: "POST",
      body: JSON.stringify({ password }),
    });
  },
  logout(): Promise<{ ok: true }> {
    return readJson<{ ok: true }>("/api/logout", {
      method: "POST",
    });
  },
  fetchSessions(): Promise<SessionsResponse> {
    return readJson<SessionsResponse>("/api/sessions");
  },
  fetchResumeCandidates(cwd: string, agentBackend: string): Promise<ResumeCandidatesResponse> {
    return readJson<ResumeCandidatesResponse>(
      `/api/session_resume_candidates?cwd=${encodeURIComponent(cwd)}&agent_backend=${encodeURIComponent(agentBackend)}`,
    );
  },
  fetchCwdSuggestions(query: string, limit = 12): Promise<CwdSuggestionsResponse> {
    return readJson<CwdSuggestionsResponse>(
      `/api/cwd_suggestions?q=${encodeURIComponent(query)}&limit=${encodeURIComponent(String(limit))}`,
    );
  },
  createSession(payload: {
    cwd: string;
    workspace_cwd?: string | null;
    agent_backend: string;
    model_provider?: string | null;
    preferred_auth_method?: string | null;
    model?: string | null;
    reasoning_effort?: string | null;
    service_tier?: string | null;
    create_in_tmux?: boolean;
    resume_session_id?: string | null;
    worktree_branch?: string | null;
  }): Promise<{ broker_pid: number }> {
    return readJson<{ broker_pid: number }>("/api/sessions", {
      method: "POST",
      body: JSON.stringify(payload),
    });
  },
  fetchTail(sessionId: string, limit = 120): Promise<TailResponse> {
    return readJson<TailResponse>(`/api/sessions/${encodeURIComponent(sessionId)}/messages/tail?limit=${limit}`);
  },
  fetchHistory(sessionId: string, cursor: string, limit = 60): Promise<HistoryResponse> {
    return readJson<HistoryResponse>(
      `/api/sessions/${encodeURIComponent(sessionId)}/messages/history?cursor=${encodeURIComponent(cursor)}&limit=${limit}`,
    );
  },
  fetchLive(sessionId: string, cursor: string): Promise<LiveResponse> {
    return readJson<LiveResponse>(
      `/api/sessions/${encodeURIComponent(sessionId)}/messages/live?cursor=${encodeURIComponent(cursor)}`,
    );
  },
  sendMessage(sessionId: string, text: string): Promise<{ ok?: true }> {
    return readJson<{ ok?: true }>(`/api/sessions/${encodeURIComponent(sessionId)}/send`, {
      method: "POST",
      body: JSON.stringify({ text }),
    });
  },
  deleteSession(sessionId: string): Promise<{ ok: true }> {
    return readJson<{ ok: true }>(`/api/sessions/${encodeURIComponent(sessionId)}/delete`, {
      method: "POST",
      body: JSON.stringify({}),
    });
  },
  renameSession(sessionId: string, name: string): Promise<{ ok?: true; alias: string }> {
    return readJson<{ ok?: true; alias: string }>(`/api/sessions/${encodeURIComponent(sessionId)}/rename`, {
      method: "POST",
      body: JSON.stringify({ name }),
    });
  },
  editSession(
    sessionId: string,
    payload: { name: string; priority_offset: number; snooze_until: number | null; dependency_session_id: string | null },
  ): Promise<EditSessionResponse> {
    return readJson<EditSessionResponse>(`/api/sessions/${encodeURIComponent(sessionId)}/edit`, {
      method: "POST",
      body: JSON.stringify(payload),
    });
  },
  enqueueMessage(sessionId: string, text: string): Promise<{ ok?: true; queued?: boolean; queue_len?: number }> {
    return readJson<{ ok?: true; queued?: boolean; queue_len?: number }>(
      `/api/sessions/${encodeURIComponent(sessionId)}/enqueue`,
      {
        method: "POST",
        body: JSON.stringify({ text }),
      },
    );
  },
  interrupt(sessionId: string): Promise<{ ok: true }> {
    return readJson<{ ok: true }>(`/api/sessions/${encodeURIComponent(sessionId)}/interrupt`, {
      method: "POST",
    });
  },
  sendTerminalResponse(sessionId: string, kind: string, value: string): Promise<{ ok: true }> {
    return readJson<{ ok: true }>(`/api/sessions/${encodeURIComponent(sessionId)}/terminal_response`, {
      method: "POST",
      body: JSON.stringify({ kind, value }),
    });
  },
  fetchDiagnostics(sessionId: string): Promise<DiagnosticsResponse> {
    return readJson<DiagnosticsResponse>(`/api/sessions/${encodeURIComponent(sessionId)}/diagnostics`);
  },
  fetchQueue(sessionId: string): Promise<QueueResponse> {
    return readJson<QueueResponse>(`/api/sessions/${encodeURIComponent(sessionId)}/queue`);
  },
  updateQueueItem(sessionId: string, itemId: string, text: string): Promise<{ ok?: true }> {
    return readJson<{ ok?: true }>(`/api/sessions/${encodeURIComponent(sessionId)}/queue/update`, {
      method: "POST",
      body: JSON.stringify({ id: itemId, text }),
    });
  },
  deleteQueueItem(sessionId: string, itemId: string): Promise<{ ok?: true }> {
    return readJson<{ ok?: true }>(`/api/sessions/${encodeURIComponent(sessionId)}/queue/delete`, {
      method: "POST",
      body: JSON.stringify({ id: itemId }),
    });
  },
  moveQueueItem(sessionId: string, itemId: string, toIndex: number): Promise<{ ok?: true }> {
    return readJson<{ ok?: true }>(`/api/sessions/${encodeURIComponent(sessionId)}/queue/move`, {
      method: "POST",
      body: JSON.stringify({ id: itemId, to_index: toIndex }),
    });
  },
  fetchHarness(sessionId: string): Promise<HarnessConfig & { ok: true }> {
    return readJson<HarnessConfig & { ok: true }>(`/api/sessions/${encodeURIComponent(sessionId)}/harness`);
  },
  saveHarness(sessionId: string, payload: HarnessConfig): Promise<HarnessConfig & { ok: true }> {
    return readJson<HarnessConfig & { ok: true }>(`/api/sessions/${encodeURIComponent(sessionId)}/harness`, {
      method: "POST",
      body: JSON.stringify(payload),
    });
  },
  fetchChangedFiles(sessionId: string): Promise<ChangedFilesResponse> {
    return readJson<ChangedFilesResponse>(`/api/sessions/${encodeURIComponent(sessionId)}/git/changed_files`);
  },
  readSessionFile(sessionId: string, path: string): Promise<FileReadResponse> {
    return readJson<FileReadResponse>(
      `/api/sessions/${encodeURIComponent(sessionId)}/file/read?path=${encodeURIComponent(path)}`,
    );
  },
  searchSessionFiles(sessionId: string, query: string, limit = 120): Promise<FileSearchResponse> {
    return readJson<FileSearchResponse>(
      `/api/sessions/${encodeURIComponent(sessionId)}/file/search?q=${encodeURIComponent(query)}&limit=${encodeURIComponent(String(limit))}`,
    );
  },
  injectFile(sessionId: string, payload: { filename: string; data_b64: string; attachment_index: number }): Promise<{ ok: true; path: string; inject_text: string }> {
    return readJson<{ ok: true; path: string; inject_text: string }>(`/api/sessions/${encodeURIComponent(sessionId)}/inject_file`, {
      method: "POST",
      body: JSON.stringify(payload),
    });
  },
  fetchVoiceSettings(): Promise<VoiceSettingsResponse> {
    return readJson<VoiceSettingsResponse>("/api/settings/voice");
  },
  saveVoiceSettings(payload: {
    tts_enabled_for_narration: boolean;
    tts_enabled_for_final_response: boolean;
    tts_base_url: string;
    tts_api_key: string;
  }): Promise<VoiceSettingsResponse> {
    return readJson<VoiceSettingsResponse>("/api/settings/voice", {
      method: "POST",
      body: JSON.stringify(payload),
    });
  },
  fetchCodexConfig(): Promise<CodexConfigResponse> {
    return readJson<CodexConfigResponse>("/api/settings/codex_config");
  },
  saveCodexConfig(text: string): Promise<CodexConfigResponse> {
    return readJson<CodexConfigResponse>("/api/settings/codex_config", {
      method: "POST",
      body: JSON.stringify({ text }),
    });
  },
  restartService(): Promise<RestartServiceResponse> {
    return readJson<RestartServiceResponse>("/api/settings/restart_service", {
      method: "POST",
      body: JSON.stringify({}),
    });
  },
  fetchNotificationSubscriptions(): Promise<NotificationSubscriptionsResponse> {
    return readJson<NotificationSubscriptionsResponse>("/api/notifications/subscription");
  },
  upsertNotificationSubscription(payload: {
    subscription: PushSubscriptionJSON;
    user_agent: string;
    device_label: string;
    device_class: string;
  }): Promise<NotificationSubscriptionsResponse> {
    return readJson<NotificationSubscriptionsResponse>("/api/notifications/subscription", {
      method: "POST",
      body: JSON.stringify(payload),
    });
  },
  toggleNotificationSubscription(endpoint: string, enabled: boolean): Promise<NotificationSubscriptionsResponse> {
    return readJson<NotificationSubscriptionsResponse>("/api/notifications/subscription/toggle", {
      method: "POST",
      body: JSON.stringify({ endpoint, enabled }),
    });
  },
  fetchNotificationFeed(since: number): Promise<{ ok: true; items: Array<Record<string, unknown>> }> {
    return readJson<{ ok: true; items: Array<Record<string, unknown>> }>(
      `/api/notifications/feed?since=${encodeURIComponent(String(since))}`,
    );
  },
  createShareLink(payload: {
    label?: string;
    session_ids: string[];
    nicknames?: Record<string, string>;
    expires_in_hours?: number;
    allow_interrupt?: boolean;
    allow_files?: boolean;
    allow_attachment_downloads?: boolean;
  }): Promise<ShareCreateResponse> {
    return readJson<ShareCreateResponse>("/api/v1/share-links", {
      method: "POST",
      body: JSON.stringify(payload),
    });
  },
  fetchShareLinks(): Promise<ShareListResponse> {
    return readJson<ShareListResponse>("/api/v1/share-links");
  },
  fetchShareLink(shareId: string): Promise<ShareSet> {
    return readJson<ShareSet>(`/api/v1/share-links/${encodeURIComponent(shareId)}`);
  },
  updateShareLink(
    shareId: string,
    payload: { label?: string; session_ids: string[]; nicknames?: Record<string, string> },
  ): Promise<ShareSet> {
    return readJson<ShareSet>(`/api/v1/share-links/${encodeURIComponent(shareId)}`, {
      method: "POST",
      body: JSON.stringify(payload),
    });
  },
  deleteShareLink(shareId: string): Promise<{ ok: true; share_id: string }> {
    return readJson<{ ok: true; share_id: string }>(`/api/v1/share-links/${encodeURIComponent(shareId)}`, {
      method: "DELETE",
    });
  },
  loginShareLink(shareId: string, password: string): Promise<ShareLoginResponse> {
    return readJson<ShareLoginResponse>(`/api/v1/share-links/${encodeURIComponent(shareId)}/login`, {
      method: "POST",
      body: JSON.stringify({ password }),
    });
  },
  fetchShareInfo(shareId: string): Promise<ShareLoginResponse> {
    return readJson<ShareLoginResponse>(`/share/${encodeURIComponent(shareId)}/info`);
  },
  fetchShareTail(shareId: string, sessionId: string, limit = 120): Promise<TailResponse> {
    return readJson<TailResponse>(
      `/share/${encodeURIComponent(shareId)}/sessions/${encodeURIComponent(sessionId)}/messages/tail?limit=${limit}`,
    );
  },
  fetchShareHistory(shareId: string, sessionId: string, cursor: string, limit = 60): Promise<HistoryResponse> {
    return readJson<HistoryResponse>(
      `/share/${encodeURIComponent(shareId)}/sessions/${encodeURIComponent(sessionId)}/messages/history?cursor=${encodeURIComponent(cursor)}&limit=${limit}`,
    );
  },
  fetchShareLive(shareId: string, sessionId: string, cursor: string): Promise<LiveResponse> {
    return readJson<LiveResponse>(
      `/share/${encodeURIComponent(shareId)}/sessions/${encodeURIComponent(sessionId)}/messages/live?cursor=${encodeURIComponent(cursor)}`,
    );
  },
  sendShareMessage(shareId: string, sessionId: string, text: string): Promise<{ ok?: true }> {
    return readJson<{ ok?: true }>(`/share/${encodeURIComponent(shareId)}/sessions/${encodeURIComponent(sessionId)}/send`, {
      method: "POST",
      body: JSON.stringify({ text }),
    });
  },
  interruptShareSession(shareId: string, sessionId: string): Promise<{ ok: true }> {
    return readJson<{ ok: true }>(`/share/${encodeURIComponent(shareId)}/sessions/${encodeURIComponent(sessionId)}/interrupt`, {
      method: "POST",
    });
  },
  readShareFile(shareId: string, sessionId: string, path: string): Promise<FileReadResponse> {
    return readJson<FileReadResponse>(
      `/share/${encodeURIComponent(shareId)}/sessions/${encodeURIComponent(sessionId)}/file/read?path=${encodeURIComponent(path)}`,
    );
  },
  fetchShareFiles(shareId: string, sessionId: string): Promise<ShareFilesResponse> {
    return readJson<ShareFilesResponse>(`/share/${encodeURIComponent(shareId)}/sessions/${encodeURIComponent(sessionId)}/files`);
  },
};
