import { workspace, window, StatusBarAlignment, StatusBarItem, ExtensionContext } from 'vscode';
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind
} from 'vscode-languageclient/node';

let client: LanguageClient;
let statusBarItem: StatusBarItem;

interface PackageStatusParams {
  name: string;
  version: string;
}

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
      { scheme: 'file', language: 'debsourceoptions' },
      { scheme: 'file', language: 'debupstreammetadata' },
      { scheme: 'file', language: 'debrules' },
    ],
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher('**/debian/{control,copyright,watch,changelog,tests/control,source/format,source/options,source/local-options,upstream/metadata,rules}')
    }
  };

  // Create status bar item for package info
  statusBarItem = window.createStatusBarItem(StatusBarAlignment.Left);
  context.subscriptions.push(statusBarItem);

  // Create the language client and start it
  client = new LanguageClient(
    'debian',
    'Debian Language Server',
    serverOptions,
    clientOptions
  );

  // Listen for package status notifications from the server
  client.onNotification('debian/packageStatus', (params: PackageStatusParams) => {
    statusBarItem.text = `$(package) ${params.name} ${params.version}`;
    statusBarItem.tooltip = `Debian package: ${params.name} ${params.version}`;
    statusBarItem.show();
  });

  // Hide status bar when switching to non-debian files
  window.onDidChangeActiveTextEditor((editor) => {
    if (!editor || !clientOptions.documentSelector) {
      statusBarItem.hide();
    }
  }, null, context.subscriptions);

  // Start the client (this will also launch the server)
  client.start();
}

export function deactivate(): Thenable<void> | undefined {
  if (statusBarItem) {
    statusBarItem.dispose();
  }
  if (!client) {
    return undefined;
  }
  return client.stop();
}
