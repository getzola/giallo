#!/usr/bin/env node

const fs = require('fs');
const path = require('path');
const { Registry, parseRawGrammar } = require('../vscode-textmate/out/main');

/**
 * Script to inspect token scopes using vscode-textmate
 * Processes all .sample files from grammars-themes/samples/
 * and saves tokenization scope output to {grammarName}.txt
 */

const GRAMMAR_DIR = 'grammars-themes/packages/tm-grammars/grammars/';
const SAMPLES_DIR = process.env.SAMPLES_DIR || 'grammars-themes/samples/';
const OUTPUT_DIR = 'src/fixtures/tokens';

async function getOniguruma() {
    const vscodeOniguruma = require('../vscode-textmate/node_modules/vscode-oniguruma');
    const wasmBin = fs.readFileSync(path.join(__dirname, '..', 'vscode-textmate', 'node_modules', 'vscode-oniguruma', 'release', 'onig.wasm')).buffer;
    await vscodeOniguruma.loadWASM(wasmBin);
    return {
        createOnigScanner(patterns) { return new vscodeOniguruma.OnigScanner(patterns); },
        createOnigString(s) { return new vscodeOniguruma.OnigString(s); }
    };
}

function scanGrammarDirectory() {
    const grammarMap = new Map();
    try {
        const files = fs.readdirSync(GRAMMAR_DIR);
        for (const file of files) {
            const fullPath = path.join(GRAMMAR_DIR, file);
            const ext = path.extname(file).toLowerCase();
            if (ext === ".json") {
                const baseName = path.basename(file, ext);
                grammarMap.set(baseName.toLowerCase(), fullPath);
            }
        }
    } catch (error) {
        console.error('❌ Error scanning grammar directory:', error.message);
        process.exit(1);
    }
    return grammarMap;
}

function extractGrammarName(sampleFilePath) {
    const fileName = path.basename(sampleFilePath);
    const match = fileName.match(/^(.+)\.sample$/);
    if (!match) {
        console.error('❌ Invalid sample file name. Expected format: {grammarName}.sample');
        process.exit(1);
    }
    return match[1].toLowerCase();
}

function scanSampleDirectory() {
    try {
        const files = fs.readdirSync(SAMPLES_DIR);
        return files
            .filter(file => file.endsWith('.sample'))
            .map(file => path.join(SAMPLES_DIR, file));
    } catch (error) {
        console.error('❌ Error scanning samples directory:', error.message);
        process.exit(1);
    }
}

function ensureOutputDirectory() {
    if (!fs.existsSync(OUTPUT_DIR)) {
        fs.mkdirSync(OUTPUT_DIR, { recursive: true });
    }
}

function truncateScope(scope, maxAtoms = 8) {
    const atoms = scope.split('.');
    if (atoms.length <= maxAtoms) {
        return scope;
    }
    return atoms.slice(0, maxAtoms).join('.');
}

async function processSingleFile(sampleFilePath, grammarMap, loadedGrammars, registry) {
    // Extract grammar name from sample file
    const grammarName = extractGrammarName(sampleFilePath);

    // Find matching grammar
    const grammarPath = grammarMap.get(grammarName);
    if (!grammarPath) {
        throw new Error(`Grammar not found for: ${grammarName}`);
    }

    // Read sample file content
    const fileContent = fs.readFileSync(sampleFilePath, 'utf8');

    // Get the grammar from the registry (already loaded)
    const targetGrammarContent = fs.readFileSync(grammarPath).toString();
    const targetRawGrammar = parseRawGrammar(targetGrammarContent, grammarPath);
    const grammar = await registry.loadGrammar(targetRawGrammar.scopeName);

    if (!grammar) {
        throw new Error(`Grammar not loaded in registry for scope: ${targetRawGrammar.scopeName}`);
    }

    // Tokenize the file content line by line (following src/tests/inspect.ts pattern)
    const lines = fileContent.split(/\r\n|\r|\n/);
    let ruleStack = null;
    let tokensByLine = [];

    for (let i = 0; i < lines.length; i++) {
        const line = lines[i];
        const r = grammar.tokenizeLine(line, ruleStack);
        ruleStack = r.ruleStack;

        // Keep tokens with line-local positions
        const lineTokens = r.tokens.map(token => ({
            startIndex: token.startIndex,
            endIndex: token.endIndex,
            scopes: token.scopes
        }));

        tokensByLine.push({
            lineNumber: i,
            line: line,
            tokens: lineTokens
        });
    }

    // Generate output string with per-line token numbering
    let output = '';
    tokensByLine.forEach(lineData => {
        lineData.tokens.forEach((token, tokenIndex) => {
            const value = lineData.line.substring(token.startIndex, token.endIndex);
            // Skip empty line tokens (those with empty content and [0-1] range)
            if (value === '' && token.startIndex === 0 && token.endIndex === 1) {
                return; // Skip this token
            }
            output += `${tokenIndex}: '${value}' (line ${lineData.lineNumber})\n`;
            token.scopes.forEach(scope => {
                const truncatedScope = truncateScope(scope, 8);
                output += `  - ${truncatedScope}\n`;
            });
            output += '\n'; // Add blank line between tokens
        });
    });

    return output;
}

async function inspectScopes() {
    try {
        // Scan for all available grammars and samples
        const grammarMap = scanGrammarDirectory();
        const sampleFiles = scanSampleDirectory();

        // Ensure output directory exists
        ensureOutputDirectory();

        console.log(`Found ${sampleFiles.length} sample files to process...`);

        // Load all grammars into a map for dependency resolution
        const loadedGrammars = new Map();
        for (const [name, path] of grammarMap.entries()) {
            const content = fs.readFileSync(path).toString();
            const rawGrammar = parseRawGrammar(content, path);
            loadedGrammars.set(rawGrammar.scopeName || name, rawGrammar);
        }

        // Set up oniguruma and create registry
        const onigLib = await getOniguruma();
        const registry = new Registry({
            onigLib: onigLib,
            loadGrammar: async (scopeName) => {
                return loadedGrammars.get(scopeName) || null;
            }
        });

        // Load all grammars into the registry upfront to support injections
        console.log(`Loading ${loadedGrammars.size} grammars into registry...`);
        for (const [scopeName, rawGrammar] of loadedGrammars.entries()) {
            try {
                await registry.addGrammar(rawGrammar);
            } catch (error) {
                console.warn(`⚠️  Failed to load grammar ${scopeName}:`, error.message);
            }
        }

        // Process each sample file
        for (const sampleFile of sampleFiles) {
            try {
                const grammarName = extractGrammarName(sampleFile);
                console.log(`Processing ${grammarName}...`);

                const output = await processSingleFile(sampleFile, grammarMap, loadedGrammars, registry);
                const outputFile = path.join(OUTPUT_DIR, `${grammarName}.txt`);
                fs.writeFileSync(outputFile, output, 'utf8');

                console.log(`✓ Generated ${outputFile}`);
            } catch (error) {
                const grammarName = extractGrammarName(sampleFile);
                console.error(`❌ Error processing ${grammarName}:`, error.message);
            }
        }

        console.log('\n✓ All files processed successfully!');
    } catch (error) {
        console.error('❌ Error:', error.message);
        process.exit(1);
    }
}

// Handle uncaught errors
process.on('uncaughtException', (error) => {
    console.error('❌ Unexpected error:', error);
    process.exit(1);
});

process.on('unhandledRejection', (reason, promise) => {
    console.error('Unhandled Rejection at:', promise, 'reason:', reason);
    process.exit(1);
});

// Run the main function
inspectScopes().catch(error => {
    console.error('❌ Unexpected error:', error);
    process.exit(1);
});