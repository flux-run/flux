export default async function (req) {
    if (globalThis.crypto) {
        console.log("crypto type:", typeof globalThis.crypto);
        console.log("crypto.subtle.digest type:", typeof globalThis.crypto.subtle.digest);
    }
    return new Response("OK");
}
