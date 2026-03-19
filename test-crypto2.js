export default async function (req) {
    console.log("crypto keys:", Object.keys(globalThis.crypto || {}));
    if (globalThis.crypto) {
        console.log("subtle keys:", Object.keys(globalThis.crypto.subtle || {}));
    }
    return new Response("OK");
}
