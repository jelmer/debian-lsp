import { workspace, ExtensionContext } from 'coc.nvim';

describe('coc-debian', () => {
  it('should register debian language server', async () => {
    const config = workspace.getConfiguration('debian');
    expect(config.get('enable')).toBe(true);
    expect(config.get('serverPath')).toBe('debian-lsp');
  });

  it('should activate on debian-control language', () => {
    const activationEvents = require('../package.json').activationEvents;
    expect(activationEvents).toContain('onLanguage:debian-control');
  });

  it('should have correct configuration properties', () => {
    const packageJson = require('../package.json');
    const config = packageJson.contributes.configuration.properties;
    
    expect(config['debian.enable']).toBeDefined();
    expect(config['debian.enable'].type).toBe('boolean');
    expect(config['debian.enable'].default).toBe(true);
    
    expect(config['debian.serverPath']).toBeDefined();
    expect(config['debian.serverPath'].type).toBe('string');
    expect(config['debian.serverPath'].default).toBe('debian-lsp');
  });
});