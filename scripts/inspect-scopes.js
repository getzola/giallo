#!/usr/bin/env node

const fs = require('fs');
const path = require('path');
const {createRegistry, getSamples} = require("./lib/textmate-common");

/**
 * Script to inspect token scopes using vscode-textmate
 * Processes all .sample files from grammars-themes/samples/
 * and saves tokenization scope output to {grammarName}.txt
 */

const OUTPUT_DIR = 'src/fixtures/tokens';

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

function processSample(grammar, lines) {
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
    const {registry, grammarMap, nameToScope} = await createRegistry();
    const samples = getSamples();

    // Ensure output directory exists
    ensureOutputDirectory();

    console.log(`Found ${samples.size} sample files to process...`);

    for (const [grammarName, lines] of samples.entries()) {
        console.log(`Processing sample ${grammarName}...`);
        if (!grammarMap.has(grammarName)) {
            console.warn(`⚠️  Failed to find grammar ${grammarName}`);
            continue;
        }
        const grammar = await registry.loadGrammar(nameToScope.get(grammarName));
        let output = processSample(grammar, lines);
        const outputFile = path.join(OUTPUT_DIR, `${grammarName}.txt`);
        fs.writeFileSync(outputFile, output, 'utf8');
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