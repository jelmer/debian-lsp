import { workspace, ExtensionContext } from 'vscode';
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind
} from 'vscode-languageclient/node';

let client: LanguageClient;

export function activate(context: ExtensionContext) {
  const config = workspace.getConfiguration('debian');
  const isEnable = config.get<boolean>('enable', true);

  if (!isEnable) {
    return;
  }

  const serverPath = config.get<string>('serverPath', 'debian-lsp');

  // Server options: spawn the debian-lsp executable
  const serverOptions: ServerOptions = {
    command: serverPath,
    args: [],
    transport: TransportKind.stdio
  };

  // Client options: define which files the server should watch
  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: 'file', language: 'debcontrol' },
      { scheme: 'file', language: 'debcopyright' },
      { scheme: 'file', language: 'debwatch' },
      { scheme: 'file', language: 'debtestscontrol' },
      { scheme: 'file', language: 'debchangelog' },
      { scheme: 'file', language: 'debsourceformat' },
      { scheme: 'file', language: 'debupstreammetadata' },
      { scheme: 'file', language: 'debrules' },
    ],
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher('**/debian/{control,copyright,watch,changelog,tests/control,source/format,upstream/metadata,rules}')
    }
  };

  // Create the language client and start it
  client = new LanguageClient(
    'debian',
    'Debian Language Server',
    serverOptions,
    clientOptions
  );

  // Start the client (this will also launch the server)
  client.start();
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}
