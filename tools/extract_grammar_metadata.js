#!/usr/bin/env node

const fs = require('fs');
const path = require('path');

try {
    // Load the grammars from the index.js file
    const { grammars } = require('../grammars-themes/packages/tm-grammars/index.js');

    console.log(`Found ${grammars.length} grammars to process...`);

    // Extract metadata for each grammar
    const metadata = grammars.map(grammar => ({
        name: grammar.name,
        aliases: grammar.aliases || [],
        scopeName: grammar.scopeName
    }));

    // Write metadata to JSON file
    const outputPath = path.join(__dirname, '..', 'grammar_metadata.json');
    fs.writeFileSync(outputPath, JSON.stringify(metadata, null, 2));

    // Print some statistics
    const totalAliases = metadata.reduce((sum, g) => sum + g.aliases.length, 0);
    const grammarsWithAliases = metadata.filter(g => g.aliases.length > 0).length;

    console.log(`✅ Successfully extracted metadata for ${metadata.length} grammars`);
    console.log(`📝 Written to: ${outputPath}`);
    console.log(`📊 Statistics:`);
    console.log(`   - Total aliases: ${totalAliases}`);
    console.log(`   - Grammars with aliases: ${grammarsWithAliases}`);
    console.log(`   - Grammars without aliases: ${metadata.length - grammarsWithAliases}`);

    // Show some examples of grammars with aliases
    const examplesWithAliases = metadata
        .filter(g => g.aliases.length > 0)
        .slice(0, 5);

    if (examplesWithAliases.length > 0) {
        console.log(`\n📋 Example grammars with aliases:`);
        examplesWithAliases.forEach(g => {
            console.log(`   - ${g.name}: [${g.aliases.join(', ')}]`);
        });
    }

} catch (error) {
    console.error('❌ Error extracting grammar metadata:', error.message);
    process.exit(1);
}