export default async function (req) {
    console.log("Testing WebCrypto API...");
    
    // Test digest
    const data = new TextEncoder().encode("flux is awesome");
    const hashBuffer = await crypto.subtle.digest("SHA-256", data);
    const hashArray = Array.from(new Uint8Array(hashBuffer));
    const hashHex = hashArray.map(b => b.toString(16).padStart(2, '0')).join('');
    console.log("SHA-256 Hash:", hashHex);

    // Test random values
    const array = new Uint32Array(1);
    crypto.getRandomValues(array);
    console.log("Random value:", array[0]);

    return new Response(JSON.stringify({ hash: hashHex, random: array[0] }));
}
