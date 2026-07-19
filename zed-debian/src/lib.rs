use zed_extension_api as zed;

struct DebianExtension;

fn get_path_to_language_server_executable(
    settings: &zed::settings::LspSettings,
    worktree: &zed::Worktree,
) -> zed::Result<String> {
    if let Some(path) = settings
        .binary
        .as_ref()
        .and_then(|binary| binary.path.clone())
    {
        return Ok(path);
    }

    if let Some(path) = worktree.which("debian") {
        return Ok(path);
    }

    Ok("debian".to_string())
}

fn get_args_for_language_server() -> zed::Result<Vec<String>> {
    Ok(Vec::new())
}

fn get_env_for_language_server() -> zed::Result<Vec<(String, String)>> {
    Ok(Vec::new())
}

impl zed::Extension for DebianExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        let settings =
            zed::settings::LspSettings::for_worktree(language_server_id.as_ref(), worktree)?;

        Ok(zed::Command {
            command: get_path_to_language_server_executable(&settings, worktree)?,
            args: get_args_for_language_server()?,
            env: get_env_for_language_server()?,
        })
    }
}

zed::register_extension!(DebianExtension);
