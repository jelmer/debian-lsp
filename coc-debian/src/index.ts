import {
  ExtensionContext,
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  services,
  workspace
} from 'coc.nvim';

/**
 * Set up highlight links for semantic token types.
 *
 * coc.nvim creates highlight groups named CocSemType<tokenType> for each
 * semantic token type reported by the server. By default only standard LSP
 * types get linked, so we link the custom debian-lsp types to Vim groups.
 */
function setupSemanticHighlights(): void {
  const { nvim } = workspace;

  const links: Record<string, string> = {
    // deb822 field types
    CocSemTypedebianField: 'Identifier',
    CocSemTypedebianUnknownField: 'PreProc',
    CocSemTypedebianValue: 'String',
    CocSemTypedebianComment: 'Comment',

    // Changelog-specific types
    CocSemTypechangelogPackage: 'Title',
    CocSemTypechangelogVersion: 'Number',
    CocSemTypechangelogDistribution: 'Constant',
    CocSemTypechangelogUrgency: 'Keyword',
    CocSemTypechangelogMaintainer: 'Special',
    CocSemTypechangelogTimestamp: 'String',
    CocSemTypechangelogMetadataValue: 'String',
  };

  for (const [group, target] of Object.entries(links)) {
    nvim.command(`hi default link ${group} ${target}`, true);
  }
}

export async function activate(context: ExtensionContext): Promise<void> {
  const config = workspace.getConfiguration('debian');
  const isEnable = config.get<boolean>('enable', true);

  if (!isEnable) {
    return;
  }

  setupSemanticHighlights();

  const serverPath = config.get<string>('serverPath', 'debian-lsp');

  const serverOptions: ServerOptions = {
    command: serverPath,
    args: []
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: 'file', pattern: '**/debian/control' },
      { scheme: 'file', pattern: '**/control' },
      { scheme: 'file', pattern: '**/debian/copyright' },
      { scheme: 'file', pattern: '**/copyright' },
      { scheme: 'file', pattern: '**/debian/watch' },
      { scheme: 'file', pattern: '**/watch' },
      { scheme: 'file', pattern: '**/debian/tests/control' },
      { scheme: 'file', pattern: '**/debian/changelog' },
      { scheme: 'file', pattern: '**/changelog' },
      { scheme: 'file', pattern: '**/debian/source/format' },
      { scheme: 'file', pattern: '**/debian/upstream/metadata' },
      { scheme: 'file', pattern: '**/debian/rules' }
    ],
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher('**/debian/{control,copyright,watch,changelog,tests/control,source/format,upstream/metadata,rules}')
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