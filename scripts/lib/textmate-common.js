const fs = require('fs');
const path = require('path');

const { Registry, parseRawGrammar } = require(path.join(__dirname, '../../vscode-textmate/out/main'));

// Shared constants
const GRAMMAR_DIR = 'grammars-themes/packages/tm-grammars/grammars/';
const SAMPLES_DIR = 'grammars-themes/samples/';


async function getOniguruma() {
    const vscodeOniguruma = require(path.join(__dirname, '../../vscode-textmate/node_modules/vscode-oniguruma'));
    const wasmBin = fs.readFileSync(path.join(__dirname, '../../vscode-textmate/node_modules/vscode-oniguruma/release/onig.wasm')).buffer;
    await vscodeOniguruma.loadWASM(wasmBin);
    return {
        createOnigScanner(patterns) { return new vscodeOniguruma.OnigScanner(patterns); },
        createOnigString(s) { return new vscodeOniguruma.OnigString(s); }
    };
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

function getSamples() {
    try {
        const files = fs.readdirSync(SAMPLES_DIR)
            .filter(file => file.endsWith('.sample'))
            .map(file => path.join(SAMPLES_DIR, file));

        const out = new Map();
        for (const file of files) {
            let grammarName = extractGrammarName(file);
            out.set(grammarName, fs.readFileSync(file, 'utf8').split(/\r\n|\r|\n/));
        }
        return out;
    } catch (error) {
        console.error('❌ Error scanning samples directory:', error.message);
        process.exit(1);
    }
}


async function createRegistry() {
    // Scan for all available grammars in default directory
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

    // Load all grammars into a map for dependency resolution
    const loadedGrammars = new Map();
    const nameToScope = new Map();
    for (const [name, grammarPath] of grammarMap.entries()) {
        try {
            const content = fs.readFileSync(grammarPath).toString();
            const rawGrammar = parseRawGrammar(content, grammarPath);
            if (rawGrammar && rawGrammar.scopeName) {
                loadedGrammars.set(rawGrammar.scopeName, rawGrammar);
                nameToScope.set(name, rawGrammar.scopeName);
            }
        } catch (error) {
            console.warn(`⚠️  Failed to parse grammar ${name}:`, error.message);
        }
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

    return {
        registry,
        grammarMap,
        loadedGrammars,
        nameToScope,
    };
}

module.exports = {
    GRAMMAR_DIR,
    SAMPLES_DIR,
    getSamples,
    createRegistry,
};