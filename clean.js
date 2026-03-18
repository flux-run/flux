const fs = require('fs');
let code = fs.readFileSync('runtime/runners/run-flux-integration.ts', 'utf8');

const toRemove = ['echo', 'json-types', 'web-apis', 'request-isolation', 'jwks-cache', 'jwt-auth', 'async-ops', 'error-handling'];

// Remove HANDLERS_DIR usage
code = code.replace(/const entryBaseDir = suite.handlerBaseDir === "examples" \? EXAMPLES_DIR : HANDLERS_DIR;/g, 'const entryBaseDir = suite.handlerBaseDir === "examples" ? EXAMPLES_DIR : __dirname;');

for (const name of toRemove) {
    // Attempt to remove the block matching { name: "..." ... } down to the closing },
    // A robust way without full parsing for this file's formatting:
    
    // Find index of `name:    "${name}"` or `name: "${name}"`
    let idx = code.search(new RegExp(`name:\\s*"${name}"`));
    if (idx !== -1) {
        // scan backwards to find the opening `{`
        let openBraces = 0;
        let startIdx = idx;
        while (startIdx > 0 && code[startIdx] !== '{') {
            startIdx--;
        }
        
        // scan back a bit more to see if there's a comment `// ── ` immediately preceding
        let prevNewline = code.lastIndexOf('\n', startIdx - 1);
        let prevLineStr = code.substring(prevNewline + 1, startIdx);
        if (prevLineStr.trim() === '') {
            let prevPrevNewline = code.lastIndexOf('\n', prevNewline - 1);
            let prevPrevLineStr = code.substring(prevPrevNewline + 1, prevNewline);
            if (prevPrevLineStr.includes('// ──')) {
                startIdx = prevPrevNewline + 1;
            }
        }

        // scan forwards to find the matching closing `}`
        let braces = 1;
        let i = code.indexOf('{', startIdx) + 1;
        while (i < code.length && braces > 0) {
            if (code[i] === '{') braces++;
            if (code[i] === '}') braces--;
            i++;
        }
        
        let endIdx = i; // This is the closing `}`
        if (code[endIdx] === ',') endIdx++; // remove trailing comma
        
        // slice out
        code = code.substring(0, startIdx) + code.substring(endIdx);
    }
}

fs.writeFileSync('runtime/runners/run-flux-integration.ts', code);
console.log('Cleaned up run-flux-integration.ts');
