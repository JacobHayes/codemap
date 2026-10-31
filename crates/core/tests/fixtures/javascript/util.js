const fs = require("fs");

function read(path) {
  try {
    return fs.readFileSync(path);
  } catch (e) {
    console.log(e);
  }
  assert(path);
  return null;
}

class Box {
  open() {
    return 1;
  }
}

module.exports = { read, Box };
