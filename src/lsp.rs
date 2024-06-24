use tower_lsp::{jsonrpc::Result, lsp_types::*, Client, LanguageServer};

use super::*;

#[derive(Debug)]
pub struct MDBookLS {
    client: Client,
    /// Join set to automatically cancel the live patcher.
    _join_set: JoinSet<ActorOutput<<LivePatcher as ActorExt>::Msg>>,
    live_patcher: ActorRef<LivePatcher>,
}

impl MDBookLS {
    pub fn new(client: Client) -> Self {
        let mut join_set = Default::default();
        let live_patcher = LivePatcher::default();
        let (tx, msg_receiver) = mpsc::channel(8);
        let (_, live_patcher) =
            live_patcher.spawn_with_channel_from_join_set(tx.clone(), msg_receiver, &mut join_set);
        Self {
            client,
            _join_set: join_set,
            live_patcher,
        }
    }
}

const OPEN_PREVIEW: &str = "open_preview";
const STOP_PREVIEW: &str = "stop_preview";

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
            .map_or_else(|| ".".into(), |folder| folder.name.into());
        debug!(?book_root, "Initializing server.");
        self.live_patcher
            .cast(LivePatcherInfo::BookRoot(book_root))
            .await
            .expect("Live patcher died.");

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec![OPEN_PREVIEW.into(), STOP_PREVIEW.into()],
                    work_done_progress_options: Default::default(),
                }),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn execute_command(&self, params: ExecuteCommandParams) -> Result<Option<Value>> {
        match params.command.as_str() {
            "open_preview" => {
                // TODO: Extract the arguments.
                let open_msg = LivePatcherInfo::OpenPreview(None);
                self.live_patcher
                    .cast(open_msg)
                    .await
                    .expect("Live patcher died.");
            }
            "stop_preview" => self
                .live_patcher
                .cast(LivePatcherInfo::StopPreview)
                .await
                .expect("Live patcher died."),
            unknown_command => {
                error!(?unknown_command, "Requested to execute");
                let message = format!("Unknown command `{unknown_command}`.");
                self.client.log_message(MessageType::ERROR, message).await;
            }
        }
        Ok(None)
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}
