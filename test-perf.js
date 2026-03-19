console.log("Start:", performance.now());
for (let i = 0; i < 1e6; i++) {} // burn some cpu
console.log("Middle:", performance.now());
setTimeout(() => {
    console.log("End:", performance.now());
}, 10);
