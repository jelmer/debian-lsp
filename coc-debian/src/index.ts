import {
  ExtensionContext,
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  services,
  workspace
} from 'coc.nvim';

export async function activate(context: ExtensionContext): Promise<void> {
  const config = workspace.getConfiguration('debian');
  const isEnable = config.get<boolean>('enable', true);
  
  if (!isEnable) {
    return;
  }

  const serverPath = config.get<string>('serverPath', 'debian-lsp');
  
  const serverOptions: ServerOptions = {
    command: serverPath,
    args: []
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: 'file', pattern: '**/debian/control' },
      { scheme: 'file', pattern: '**/control' }
    ],
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher('**/debian/control')
    }
  };

  const client = new LanguageClient(
    'debian',
    'Debian Language Server',
    serverOptions,
    clientOptions
  );

  context.subscriptions.push(services.registLanguageClient(client));
}