# Is the recorder its own process

The answer is what `processes` assumes. The other answer would be a part of the engine program
outside the core. Either way, a classifier must never stop the core: `classifier-never-stops-core`.
