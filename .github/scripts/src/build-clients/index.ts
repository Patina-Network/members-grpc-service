import yargs from "yargs";
import { hideBin } from "yargs/helpers";

import { compileClients } from "./compile";

const { version } = await yargs(hideBin(process.argv))
  .version(false)
  .option("version", {
    type: "string",
    describe: "Release tag to publish the clients as, e.g. 1.2.34",
    demandOption: true,
  })
  .strict()
  .parse();

async function main() {
  await compileClients(version);
}

main()
  .then(() => {
    process.exit(0);
  })
  .catch((e) => {
    console.error(e);
    process.exit(1);
  });
