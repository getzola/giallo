#!/usr/bin/env node

const fs = require('fs');
const path = require('path');
const { Registry, parseRawGrammar } = require('./out/main');
const { EncodedTokenAttributes } = require('./out/encodedTokenAttributes');

/**
 * Script to generate tokenized output using vscode-textmate
 * Usage: node generate-snapshots.js <grammarFolder> <sampleFolder> <themePath> <outputDirPath>
 *
 * Requirements:
 * - grammars: JSON format only
 * - theme: JSON format only
 */

async function getOniguruma() {
    const vscodeOniguruma = require('vscode-oniguruma');
    const wasmBin = fs.readFileSync(path.join(__dirname, 'node_modules/vscode-oniguruma/release/onig.wasm')).buffer;
    await vscodeOniguruma.loadWASM(wasmBin);
    return {
        createOnigScanner(patterns) { return new vscodeOniguruma.OnigScanner(patterns); },
        createOnigString(s) { return new vscodeOniguruma.OnigString(s); }
    };
}

function formatTokenOutput(tokenText, foregroundColor, fontStyle) {
    // Format color - pad to 10 characters
    const colorPadded = (foregroundColor || '#000000').padEnd(10);

    // Format font style - create abbreviation and pad to 6 characters
    let fontStyleAbbr = '';
    if (!fontStyle || fontStyle === 0) {
        fontStyleAbbr = '      '; // 6 spaces for no style
    } else {
        fontStyleAbbr = '[';
        if (fontStyle & 2) fontStyleAbbr += 'b'; // Bold
        if (fontStyle & 1) fontStyleAbbr += 'i'; // Italic
        if (fontStyle & 4) fontStyleAbbr += 'u'; // Underline
        if (fontStyle & 8) fontStyleAbbr += 's'; // Strikethrough
        fontStyleAbbr += ']';
        fontStyleAbbr = fontStyleAbbr.padEnd(6);
    }

    return `${colorPadded}${fontStyleAbbr}${tokenText}\n`;
}

async function main() {
    console.log('Starting tokenization script...');

    // Parse command line arguments
    const args = process.argv.slice(2);
    if (args.length !== 4) {
        console.error('Usage: node generate-tokens.js <grammarFolder> <sampleFolder> <themePath> <outputDirPath>');
        process.exit(1);
    }

    const [grammarFolder, sampleFolder, themePath, outputDirPath] = args.map(p => path.resolve(p));

    // Validate that required paths exist
    if (!fs.existsSync(grammarFolder)) {
        console.error(`Grammar folder does not exist: ${grammarFolder}`);
        process.exit(1);
    }
    if (!fs.existsSync(sampleFolder)) {
        console.error(`Sample folder does not exist: ${sampleFolder}`);
        process.exit(1);
    }
    if (!fs.existsSync(themePath)) {
        console.error(`Theme file does not exist: ${themePath}`);
        process.exit(1);
    }

    // Create output directory if it doesn't exist
    if (!fs.existsSync(outputDirPath)) {
        fs.mkdirSync(outputDirPath, { recursive: true });
        console.log(`Created output directory: ${outputDirPath}`);
    }

    console.log(`Grammar folder: ${grammarFolder}`);
    console.log(`Sample folder: ${sampleFolder}`);
    console.log(`Theme path: ${themePath}`);
    console.log(`Output directory: ${outputDirPath}`);

    // Load JSON grammars
    console.log('Loading grammars...');
    const loadedGrammars = new Map();
    const grammarNameToScope = new Map();

    const grammarFiles = fs.readdirSync(grammarFolder).filter(file => file.endsWith('.json'));
    console.log(`Found ${grammarFiles.length} grammar files`);

    for (const file of grammarFiles) {
        try {
            const grammarPath = path.join(grammarFolder, file);
            const content = fs.readFileSync(grammarPath, 'utf8');
            const rawGrammar = parseRawGrammar(content, grammarPath);

            if (rawGrammar && rawGrammar.scopeName) {
                loadedGrammars.set(rawGrammar.scopeName, rawGrammar);

                // Extract grammar name from filename (e.g., "javascript.json" -> "javascript")
                const grammarName = path.basename(file, '.json');
                grammarNameToScope.set(grammarName, rawGrammar.scopeName);
            }
        } catch (error) {
            console.warn(`  Failed to load grammar ${file}:`, error.message);
        }
    }

    // Set up oniguruma and create registry
    console.log('Setting up registry...');
    const onigLib = await getOniguruma();

    const registry = new Registry({
        onigLib,
        loadGrammar: async (scopeName) => {
            return loadedGrammars.get(scopeName) || null;
        }
    });

    // Pre-load all grammars to support dependencies and injections
    for (const [scopeName, rawGrammar] of loadedGrammars.entries()) {
        try {
            await registry.addGrammar(rawGrammar);
        } catch (error) {
            console.warn(`  Failed to add grammar ${scopeName}:`, error.message);
        }
    }

    // Load and apply theme
    console.log('Loading theme...');
    try {
        const themeContent = fs.readFileSync(themePath, 'utf8');
        let theme = JSON.parse(themeContent);

        // Convert VS Code format to TextMate format if needed
        if (theme.tokenColors && !theme.settings) {
            const defaultSetting = {
                settings: {
                    foreground: theme.colors && theme.colors['editor.foreground']
                }
            };
            const themeSettings = [defaultSetting, ...theme.tokenColors];

            theme = {
                name: theme.name || path.basename(themePath, '.json'),
                settings: themeSettings
            };
        }

        registry.setTheme(theme);
        console.log(`  Theme loaded: ${path.basename(themePath)}`);
    } catch (error) {
        console.error(`Failed to load theme ${themePath}:`, error.message);
        process.exit(1);
    }

    // Get color map for converting color IDs to hex colors
    const colorMap = registry.getColorMap();

    // Process sample files
    console.log('Processing sample files...');
    const sampleFiles = fs.readdirSync(sampleFolder);
    console.log(`Found ${sampleFiles.length} sample files`);

    for (const sampleFile of sampleFiles) {
        try {
            // Extract grammar name from filename (e.g., "javascript.txt" -> "javascript")
            const grammarName = path.basename(sampleFile, path.extname(sampleFile));

            // Find matching grammar scope
            const scopeName = grammarNameToScope.get(grammarName);
            if (!scopeName) {
                console.warn(`${sampleFile}: No matching grammar found`);
                continue;
            }

            // Load the grammar
            const grammar = await registry.loadGrammar(scopeName);
            if (!grammar) {
                console.warn(`${sampleFile}: Failed to load grammar ${scopeName}`);
                continue;
            }

            // Read sample content
            const samplePath = path.join(sampleFolder, sampleFile);
            const content = fs.readFileSync(samplePath, 'utf8');

            // Tokenize content line by line using dual approach
            const lines = content.split(/\r\n|\r|\n/);
            let ruleStack = null;
            let binaryRuleStack = null;
            let output = '';

            for (let lineIndex = 0; lineIndex < lines.length; lineIndex++) {
                const line = lines[lineIndex];

                // Call both tokenization methods
                const t1Result = grammar.tokenizeLine(line, ruleStack);
                const t2Result = grammar.tokenizeLine2(line, binaryRuleStack);

                // Parse t2 binary tokens into simple structure
                const t2TokensParsed = [];
                for (let i = 0; i < t2Result.tokens.length / 2; i++) {
                    const startIndex = t2Result.tokens[2 * i];
                    const endIndex = i + 1 < t2Result.tokens.length / 2
                        ? t2Result.tokens[2 * (i + 1)]
                        : line.length + 1;
                    const metadata = t2Result.tokens[2 * i + 1];

                    const foregroundId = EncodedTokenAttributes.getForeground(metadata);
                    const foregroundColor = colorMap[foregroundId];
                    const fontStyle = EncodedTokenAttributes.getFontStyle(metadata);
                    t2TokensParsed.push({
                        startIndex,
                        endIndex,
                        foregroundColor,
                        fontStyle,
                        metadata,
                    });
                }

                // Map t2 styling to t1 tokens using boundary checking
                for (const t1Token of t1Result.tokens) {
                    // Get token text from t1 boundaries
                    const tokenText = line.substring(t1Token.startIndex, t1Token.endIndex);
                    if (tokenText === '' && t1Token.startIndex === 0 && t1Token.endIndex === 1) {
                        continue; // Skip this token
                    }
                    let matched = false;
                    for (const t2Token of t2TokensParsed) {
                        if (t1Token.startIndex >= t2Token.startIndex &&
                            t1Token.endIndex <= t2Token.endIndex) {

                            // Format token output with t1 granularity + t2 styling
                            output += formatTokenOutput(tokenText, t2Token.foregroundColor, t2Token.fontStyle);
                            matched = true;
                            break;
                        }
                    }
                }

                ruleStack = t1Result.ruleStack;
                binaryRuleStack = t2Result.ruleStack;
            }

            // Write output file
            const outputFileName = `${grammarName}.txt`;
            const outputPath = path.join(outputDirPath, outputFileName);
            fs.writeFileSync(outputPath, output, 'utf8');
            console.log(`${sampleFile}`);

        } catch (error) {
            console.error(`${sampleFile}: Error - ${error.message}`);
        }
    }

    console.log('\nScript completed successfully!');
}

// Handle uncaught errors
process.on('uncaughtException', (error) => {
    console.error('Uncaught Exception:', error);
    process.exit(1);
});

process.on('unhandledRejection', (reason, promise) => {
    console.error('Unhandled Rejection at:', promise, 'reason:', reason);
    process.exit(1);
});

// Run the main function
main().catch(console.error);