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
    CocSemTypechangelogBugReference: 'Underlined',
  };

  for (const [group, target] of Object.entries(links)) {
    nvim.command(`hi default link ${group} ${target}`, true);
  }
}

/**
 * Register autocmds to set Vim filetypes for Debian packaging files.
 *
 * coc.nvim matches documents to language servers using Vim filetypes
 * (the `language` field in DocumentFilter). Without these autocmds,
 * files like debian/upstream/metadata would not get a recognized
 * filetype and the language server would not attach.
 */
function setupFiletypeDetection(context: ExtensionContext): void {
  const filetypeMap: Array<[string, string]> = [
    ['*/debian/control', 'debcontrol'],
    ['*/debian/copyright', 'debcopyright'],
    ['*/debian/watch', 'debwatch'],
    ['*/debian/tests/control', 'autopkgtest'],
    ['*/debian/changelog', 'debchangelog'],
    ['*/debian/changelog.dch', 'debchangelog'],
    ['*/debian/source/format', 'debsources'],
    ['*/debian/source/options', 'debsourceoptions'],
    ['*/debian/source/local-options', 'debsourceoptions'],
    ['*/debian/upstream/metadata', 'debupstream'],
    ['*/debian/rules', 'debrules'],
    ['*/debian/patches/series', 'debpatches'],
  ];

  for (const [pattern, filetype] of filetypeMap) {
    context.subscriptions.push(
      workspace.registerAutocmd({
        event: ['BufNewFile', 'BufRead'],
        pattern,
        callback: () => {
          const { nvim } = workspace;
          nvim.command(`setfiletype ${filetype}`, true);
        },
      })
    );
  }
}

export async function activate(context: ExtensionContext): Promise<void> {
  const config = workspace.getConfiguration('debian');
  const isEnable = config.get<boolean>('enable', true);

  if (!isEnable) {
    return;
  }

  setupSemanticHighlights();
  setupFiletypeDetection(context);

  const serverPath = config.get<string>('serverPath', 'debian-lsp');

  const serverOptions: ServerOptions = {
    command: serverPath,
    args: []
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { language: 'debcontrol' },
      { language: 'debcopyright' },
      { language: 'debwatch' },
      { language: 'autopkgtest' },
      { language: 'debchangelog' },
      { language: 'debsources' },
      { language: 'debsourceoptions' },
      { language: 'debupstream' },
      { language: 'debrules' },
      { language: 'debpatches' },
      { scheme: 'file', pattern: '**/debian/control' },
      { scheme: 'file', pattern: '**/debian/copyright' },
      { scheme: 'file', pattern: '**/debian/watch' },
      { scheme: 'file', pattern: '**/debian/tests/control' },
      { scheme: 'file', pattern: '**/debian/changelog' },
      { scheme: 'file', pattern: '**/debian/changelog.dch' },
      { scheme: 'file', pattern: '**/debian/source/format' },
      { scheme: 'file', pattern: '**/debian/source/options' },
      { scheme: 'file', pattern: '**/debian/source/local-options' },
      { scheme: 'file', pattern: '**/debian/upstream/metadata' },
      { scheme: 'file', pattern: '**/debian/rules' },
      { scheme: 'file', pattern: '**/debian/patches/series' },
    ],
    initializationOptions: {
      upstreamOntologistNetAccess: config.get<boolean>('upstreamOntologistNetAccess', false),
    },
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher('**/debian/{control,copyright,watch,changelog,changelog.dch,tests/control,source/format,source/options,source/local-options,upstream/metadata,rules,patches/series}')
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
