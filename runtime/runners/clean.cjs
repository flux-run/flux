const ts = require('typescript');
const fs = require('fs');
const file = 'run-flux-integration.ts';

let code = fs.readFileSync(file, 'utf8');
const sourceFile = ts.createSourceFile(file, code, ts.ScriptTarget.Latest, true);

let arrPos = -1;
let arrEnd = -1;
let keepNodes = [];

function visit(node) {
    if (ts.isVariableDeclaration(node) && node.name.text === 'SUITES') {
        const arr = node.initializer;
        if (arr && ts.isArrayLiteralExpression(arr)) {
            arrPos = arr.pos;
            arrEnd = arr.end;
            
            for (const el of arr.elements) {
                let keep = false;
                if (ts.isObjectLiteralExpression(el)) {
                    for (const prop of el.properties) {
                        if (ts.isPropertyAssignment(prop) && prop.name && prop.name.text === 'handlerBaseDir') {
                            if (ts.isStringLiteral(prop.initializer) && prop.initializer.text === 'examples') {
                                keep = true;
                            }
                        }
                    }
                }
                if (keep) {
                    keepNodes.push(el);
                }
            }
        }
    }
    ts.forEachChild(node, visit);
}

visit(sourceFile);

if (arrPos !== -1) {
    const newElementsCode = keepNodes.map(el => code.substring(el.pos, el.end)).join(',\n');
    const newArrayCode = `[${newElementsCode}\n]`;
    code = code.substring(0, arrPos) + newArrayCode + code.substring(arrEnd);
}

code = code.replace(/const entryBaseDir = suite\.handlerBaseDir === "examples" \? EXAMPLES_DIR : HANDLERS_DIR;/g, 'const entryBaseDir = suite.handlerBaseDir === "examples" ? EXAMPLES_DIR : __dirname;');
code = code.replace(/const JWKS_SERVER_ENTRY = [^\n]+/g, '// jwks entry removed');

fs.writeFileSync(file, code);
console.log("Successfully filtered SUITES.");
