#pragma once
#include "asiodrivers.h"
#include "asio.h"

// Helper function to call destructors from within library
extern "C" void destruct_AsioDrivers(AsioDrivers * a);

// Helper function to wrap confusing preprocessor
extern "C" ASIOError get_sample_rate(double * rate);
