import { crypto } from "ext:deno_crypto/00_crypto.js";
export default async function (req) {
    if (crypto) {
        console.log("IMPORTED CRYPTO!");
        return new Response("Found crypto!");
    }
    return new Response("No crypto");
}
