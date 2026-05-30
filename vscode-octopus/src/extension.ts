import * as vscode from 'vscode';

export function activate(context: vscode.ExtensionContext) {
    // Register YAML schema associations if the YAML extension is available
    const yamlExtension = vscode.extensions.getExtension('redhat.vscode-yaml');

    if (yamlExtension) {
        configureYamlSchema(context);
    }

    // Status bar item shown when editing Octopus config files
    const statusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
    statusBar.text = '$(server) Octopus';
    statusBar.tooltip = 'Octopus API Gateway Config';
    context.subscriptions.push(statusBar);

    // Show/hide status bar based on active editor
    const updateStatusBar = (editor: vscode.TextEditor | undefined) => {
        if (editor && isOctopusConfig(editor.document.fileName)) {
            statusBar.show();
        } else {
            statusBar.hide();
        }
    };

    context.subscriptions.push(
        vscode.window.onDidChangeActiveTextEditor(updateStatusBar)
    );

    // Check the current active editor on activation
    updateStatusBar(vscode.window.activeTextEditor);
}

/**
 * Check whether a file name matches known Octopus configuration patterns.
 */
function isOctopusConfig(fileName: string): boolean {
    const base = fileName.replace(/\\/g, '/').split('/').pop()?.toLowerCase() ?? '';
    const octopusPatterns = [
        /^octopus[.\-]/,
        /^config\.yaml$/,
        /^config\..+\.yaml$/,
        /^config\.yml$/,
        /^config\..+\.yml$/,
        /\.octopus\.json$/,
        /^octopus-schema\.json$/,
        /^octopus-gen\.yaml$/,
        /^octopus-gen\..+\.yaml$/,
    ];
    return octopusPatterns.some(p => p.test(base));
}

/**
 * Register schema associations with the Red Hat YAML extension so that
 * IntelliSense and validation work automatically.
 */
function configureYamlSchema(context: vscode.ExtensionContext) {
    const config = vscode.workspace.getConfiguration('yaml');
    const schemas = config.get<Record<string, string | string[]>>('schemas') || {};

    const configSchemaPath = vscode.Uri.joinPath(
        context.extensionUri, 'schema', 'octopus-config.schema.json'
    ).toString();

    const genSchemaPath = vscode.Uri.joinPath(
        context.extensionUri, 'schema', 'octopus-gen.schema.json'
    ).toString();

    // Associate schemas with matching file patterns
    schemas[configSchemaPath] = [
        'config.yaml',
        'config.*.yaml',
        'octopus.yaml',
        'octopus.*.yaml',
        'octopus-config.yaml',
        'octopus-config.*.yaml',
    ];

    schemas[genSchemaPath] = [
        'octopus-gen.yaml',
        'octopus-gen.*.yaml',
    ];

    config.update('schemas', schemas, vscode.ConfigurationTarget.Workspace);
}

export function deactivate() {}
