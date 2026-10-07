use tokio::task::JoinHandle;
use tower_lsp::{jsonrpc::Result, lsp_types::*, Client, LanguageServer};

use super::*;

pub struct MDBookLS {
    client: Client,
    live_patcher_handle: JoinHandle<ActorRunResult<Previewer>>,
    live_patcher: ActorRef<Previewer>,
}

impl MDBookLS {
    pub fn new(client: Client, live_patcher: Previewer) -> Self {
        let (reverse_tx, mut reverse_rx) = mpsc::channel::<SearchLocation>(8);
        let notification_client = client.clone();
        tokio::spawn(async move {
            while let Some(location) = reverse_rx.recv().await {
                if let Ok(uri) = Url::from_file_path(location.path) {
                    notification_client
                        .send_notification::<ReverseSearch>(serde_json::json!({
                            "uri": uri,
                            "position": {"line": location.line, "character": location.character},
                        }))
                        .await;
                }
            }
        });
        let live_patcher = live_patcher.with_reverse_search(reverse_tx);
        let (tx, msg_receiver) = mpsc::channel(8);
        let (live_patcher_handle, live_patcher) =
            live_patcher.spawn_with_channel(tx.clone(), msg_receiver);
        Self {
            client,
            live_patcher_handle,
            live_patcher,
        }
    }
}

const OPEN_PREVIEW: &str = "open_preview";
const STOP_PREVIEW: &str = "stop_preview";
const FORWARD_SEARCH: &str = "forward_search";

// 🧑 "implement forward and reverse searches and integrate with nvim and vscode"
enum ReverseSearch {}
impl notification::Notification for ReverseSearch {
    type Params = Value;
    const METHOD: &'static str = "mdbook/reverseSearch";
}

#[tower_lsp::async_trait]
impl LanguageServer for MDBookLS {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let book_root: PathBuf = params
            .workspace_folders
            .and_then(|folders| {
                if folders.len() > 1 {
                    warn!(
                        ?folders,
                        "More than one workspace folder detected. Only using the first one."
                    );
                }
                folders.into_iter().next()
            })
            .and_then(|folder| folder.uri.to_file_path().ok())
            .or_else(|| params.root_uri.and_then(|uri| uri.to_file_path().ok()))
            .unwrap_or_else(|| ".".into());
        debug!(?book_root, "Initializing server.");
        self.live_patcher
            .cast(PreviewInfo::BookRoot(book_root))
            .await
            .expect("Live patcher died.");

        Ok(InitializeResult {
            capabilities: server_capabilities(),
            ..Default::default()
        })
    }

    async fn execute_command(&self, params: ExecuteCommandParams) -> Result<Option<Value>> {
        match params.command.as_str() {
            "open_preview" => {
                let open_msg = open_params(params);
                self.live_patcher
                    .cast(open_msg)
                    .await
                    .expect("Live patcher died.");
            }
            "stop_preview" => self
                .live_patcher
                .cast(PreviewInfo::StopPreview)
                .await
                .expect("Live patcher died."),
            FORWARD_SEARCH => {
                let path = params
                    .arguments
                    .first()
                    .and_then(Value::as_str)
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute())
                    .ok_or_else(|| {
                        tower_lsp::jsonrpc::Error::invalid_params("expected absolute source path")
                    })?;
                let position: Position =
                    serde_json::from_value(params.arguments.get(1).cloned().ok_or_else(|| {
                        tower_lsp::jsonrpc::Error::invalid_params("expected source position")
                    })?)
                    .map_err(|_| {
                        tower_lsp::jsonrpc::Error::invalid_params("expected source position")
                    })?;
                self.live_patcher
                    .cast(PreviewInfo::ForwardSearch {
                        path,
                        line: position.line,
                        character: position.character,
                    })
                    .await
                    .map_err(|_| tower_lsp::jsonrpc::Error::internal_error())?;
            }
            unknown_command => {
                error!(?unknown_command, "Requested to execute");
                let message = format!("Unknown command `{unknown_command}`.");
                self.client.log_message(MessageType::ERROR, message).await;
            }
        }
        Ok(None)
    }

    async fn did_open(
        &self,
        DidOpenTextDocumentParams {
            text_document:
                TextDocumentItem {
                    uri,
                    language_id,
                    version,
                    text,
                },
        }: DidOpenTextDocumentParams,
    ) {
        info!(uri.path = uri.path(), language_id, version, "did_open");
        match (language_id.as_str(), uri2abs_file_path(&uri)) {
            ("markdown", Some(path)) => {
                let msg = PreviewInfo::Opened {
                    path,
                    version,
                    content: text,
                };
                let task = self.live_patcher.cast(msg);
                task.await.expect("LivePatcher died.");
            }
            ("markdown", _) => info!(uri.path = uri.path(), "Markdown but not a file!"),
            _ => {}
        }
    }

    async fn did_change(
        &self,
        DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier { uri, version },
            mut content_changes,
        }: DidChangeTextDocumentParams,
    ) {
        info!(uri.path = uri.path(), version, "did_change");
        match (content_changes.pop(), uri2abs_file_path(&uri)) {
            (Some(TextDocumentContentChangeEvent { text, .. }), Some(path)) => {
                let msg = PreviewInfo::ModifiedContent {
                    path,
                    version,
                    content: text,
                };
                let task = self.live_patcher.cast(msg);
                task.await.expect("LivePatcher died.");
            }
            (Some(_), _) => info!(uri.path = uri.path(), "Not a file!"),
            _ => warn!("Empty content change!"),
        }
        if !content_changes.is_empty() {
            warn!(
                ?content_changes,
                "Unexpected due to `TextDocumentSyncKind::FULL`: more than one content changes! Only handled the last one."
            );
        }
    }

    async fn did_close(
        &self,
        DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri },
        }: DidCloseTextDocumentParams,
    ) {
        info!(uri.path = uri.path(), "did_close");
        if let Some(path) = uri2abs_file_path(&uri) {
            let msg = PreviewInfo::Closed(path);
            let task = self.live_patcher.cast(msg);
            task.await.expect("LivePatcher died.");
        }
    }

    async fn shutdown(&self) -> Result<()> {
        self.live_patcher.cancel();
        Ok(())
    }
}

fn open_params(params: ExecuteCommandParams) -> PreviewInfo {
    let mut args = params.arguments.into_iter();
    let socket_address = args.next().and_then(|v| {
        v.as_str().and_then(|s| {
            s.parse::<SocketAddr>()
                .map_err(|err| error!(?err, ?s, "Parsing socket address in open params."))
                .ok()
        })
    });
    let open_browser_at = args.next().and_then(|v| v.as_str().map(PathBuf::from));
    PreviewInfo::OpenPreview {
        socket_address,
        open_browser_at,
    }
}

impl Drop for MDBookLS {
    fn drop(&mut self) {
        self.live_patcher_handle.abort();
    }
}

fn uri2abs_file_path(uri: &Url) -> Option<PathBuf> {
    uri.to_file_path().ok()
}

fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        // NOTE: Let the client send the whole file on every change so
        // we do not need to patch it ourselves.
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        execute_command_provider: Some(ExecuteCommandOptions {
            commands: vec![
                OPEN_PREVIEW.into(),
                STOP_PREVIEW.into(),
                FORWARD_SEARCH.into(),
            ],
            work_done_progress_options: Default::default(),
        }),
        ..Default::default()
    }
}
