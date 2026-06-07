from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_share_sidebar_prefers_workspace_cwd_over_real_cwd():
    source = (ROOT / "frontend/src/share.tsx").read_text()
    assert "{item.workspace_cwd || item.cwd || item.session_id}" in source
    assert "{item.cwd || item.workspace_cwd || item.session_id}" not in source


def test_share_workspace_has_tool_visibility_toggle():
    app_source = (ROOT / "frontend/src/app.tsx").read_text()
    share_source = (ROOT / "frontend/src/share.tsx").read_text()

    assert "const visibleShareTranscript = useMemo(" in app_source
    assert "shareTranscript.filter(transcriptEventVisibleWithToolsHidden)" in app_source
    assert "transcript={visibleShareTranscript}" in app_source
    assert "showTools={showTools}" in app_source
    assert "onToggleTools={() => setShowTools((current) => !current)}" in app_source

    assert '"schedule" | "tools"' in share_source
    assert 'title={props.showTools ? "Hide tools and narration" : "Show tools and narration"}' in share_source
    assert "aria-pressed={props.showTools}" in share_source


def test_share_sidebar_has_runtime_status_dot():
    share_source = (ROOT / "frontend/src/share.tsx").read_text()
    css_source = (ROOT / "frontend/src/styles.css").read_text()

    assert "const itemBusy = isCurrentSession ? Boolean(props.busy || item.busy) : Boolean(item.busy);" in share_source
    assert 'const itemStatus = itemBusy ? "working" : itemQueueLen ? `queue ${itemQueueLen}` : "idle";' in share_source
    assert 'const itemDotClass = itemBusy ? "running" : itemQueueLen ? "waiting" : "idle";' in share_source
    assert '<span className={`status-dot ${itemDotClass}`} title={itemStatus} aria-label={itemStatus} />' in share_source
    assert ".shareSession .workspaceTitleRow" in css_source


def test_share_switch_restores_cached_view_and_background_loads_files():
    source = (ROOT / "frontend/src/app.tsx").read_text()

    assert "type ShareViewSnapshot = {" in source
    assert "const shareViewCacheRef = useRef<Record<string, ShareViewSnapshot>>({});" in source
    assert "function rememberActiveShareSnapshot(sessionId = shareSessionRef.current)" in source
    assert "function restoreShareSnapshot(sessionId: string)" in source
    assert "rememberActiveShareSnapshot();" in source
    assert "if (!restoreShareSnapshot(sessionId))" in source
    assert "void loadShareSession(sessionId);" in source
    assert "void loadShareFilesForSession(nextSessionId);" in source
    assert "updateShareRuntimeFromResponse(nextSessionId, tail);\n      const files = await api.fetchShareFiles" not in source


def test_share_runtime_update_skips_unchanged_sidebar_state():
    source = (ROOT / "frontend/src/app.tsx").read_text()

    assert "const existing = current.sessions.find((session) => session.session_id === sessionId);" in source
    assert "if (existing && Boolean(existing.busy) === busyValue && Number(existing.queue_len || 0) === queueLength) return current;" in source


def test_share_workspace_uses_shared_session_queue_api():
    app_source = (ROOT / "frontend/src/app.tsx").read_text()
    api_source = (ROOT / "frontend/src/lib/api.ts").read_text()
    share_source = (ROOT / "frontend/src/share.tsx").read_text()
    routes_source = (ROOT / "backend-rs/src/routes.rs").read_text()

    assert "fetchShareQueue(shareId: string, sessionId: string): Promise<QueueResponse>" in api_source
    assert "enqueueShareMessage(shareId: string, sessionId: string, text: string)" in api_source
    assert "updateShareQueueItem(shareId: string, sessionId: string, itemId: string, text: string)" in api_source
    assert "deleteShareQueueItem(shareId: string, sessionId: string, itemId: string)" in api_source
    assert "moveShareQueueItem(shareId: string, sessionId: string, itemId: string, toIndex: number)" in api_source

    assert '"/share/:share_id/sessions/:session_id/queue"' in routes_source
    assert '"/share/:share_id/sessions/:session_id/enqueue"' in routes_source
    assert "async fn share_session_queue" in routes_source
    assert "async fn share_session_enqueue" in routes_source
    assert "async fn share_queue_update" in routes_source
    assert "async fn share_queue_delete" in routes_source
    assert "async fn share_queue_move" in routes_source

    assert "const [shareQueueItems, setShareQueueItems] = useState<QueueItem[]>([]);" in app_source
    assert "async function loadShareQueue(sessionId = shareSessionRef.current)" in app_source
    assert "async function enqueueShareDraft()" in app_source
    assert "onEnqueue={enqueueShareDraft}" in app_source
    assert "onOpenQueue={openShareQueueModal}" in app_source

    assert "queueItems: QueueItem[];" in share_source
    assert "onEnqueue: () => void | Promise<void>;" in share_source
    assert 'title="Queue message"' in share_source
    assert "props.queueItems.map((item, index) =>" in share_source


def test_owner_can_update_existing_share_sessions_without_recreating_link():
    source = (ROOT / "frontend/src/app.tsx").read_text()

    assert "const [editingShareId, setEditingShareId] = useState(\"\");" in source
    assert "function editManagedShare(share: ManagedShareSet)" in source
    assert "await api.updateShareLink(editingShareId" in source
    assert "Editing an existing share keeps its URL and password." in source
    assert "shareManagerEditBtn" in source
    assert "编辑会话" in source


def test_owner_share_manager_has_clear_delete_button_and_refreshes_after_delete():
    source = (ROOT / "frontend/src/app.tsx").read_text()

    assert "void refreshManagedShares();" in source
    assert 'title="Delete share"' in source
    assert 'Deleting..." : "删除"' in source


def test_share_workspace_refreshes_share_info_for_membership_changes():
    source = (ROOT / "frontend/src/app.tsx").read_text()

    assert "const info = await api.fetchShareInfo(shareTarget.shareId);" in source
    assert "Unable to refresh share sessions" in source
    assert "if (shareSessionRef.current && !nextSessionIds.has(shareSessionRef.current))" in source
