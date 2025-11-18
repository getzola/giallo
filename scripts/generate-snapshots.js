#!/usr/bin/env node

const fs = require('fs');
const path = require('path');
const {createRegistry, getSamples} = require("./lib/textmate-common");
const { EncodedTokenAttributes } = require(path.join(__dirname, '../vscode-textmate/out/encodedTokenAttributes'));


const OUTPUT_DIR = 'src/fixtures/snapshots';

function ensureOutputDirectory() {
    if (!fs.existsSync(OUTPUT_DIR)) {
        fs.mkdirSync(OUTPUT_DIR, { recursive: true });
    }
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
    const {registry, grammarMap, nameToScope} = await createRegistry();
    const samples = getSamples();
    // Get color map for converting color IDs to hex colors
    const colorMap = registry.getColorMap();

    ensureOutputDirectory()

    for (const [grammarName, lines] of samples.entries()) {
        if (!grammarMap.has(grammarName)) {
            console.warn(`⚠️  Failed to find grammar ${grammarName}`);
            continue;
        }
        const grammar = await registry.loadGrammar(nameToScope.get(grammarName));
        let ruleStack = null;
        let binaryRuleStack = null;
        let output = '';

        for (let lineIndex = 0; lineIndex < lines.length; lineIndex++) {
            const line = lines[lineIndex];

            // Call both tokenization methods
            // We have the same output as tokenizeLine but we need tokenizeLine2 to get the style
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

            for (const t1Token of t1Result.tokens) {
                // Get token text from t1 boundaries
                const tokenText = line.substring(t1Token.startIndex, t1Token.endIndex);
                if (tokenText === '' && t1Token.startIndex === 0 && t1Token.endIndex === 1) {
                    continue; // Skip empty lines
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
        fs.writeFileSync(path.join(OUTPUT_DIR, `${grammarName}.txt`), output, 'utf8');
    }
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