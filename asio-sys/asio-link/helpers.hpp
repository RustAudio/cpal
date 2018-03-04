#pragma once
#include "asiodrivers.h"

// Helper function to call destructors from within library
extern "C" void destruct_AsioDrivers(AsioDrivers * a);
